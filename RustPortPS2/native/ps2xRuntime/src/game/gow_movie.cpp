// GOW-Port: FMV: skip / override / bonus-movie debug hooks and the fixes of the movie player (video ring, stream
// service pacing).

#include "gow_game.h"
#include "gow_guest_functions.h"
#include "runtime/ee_scheduler.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

namespace gow_game
{
    namespace
    {
        // GOW-Port: GOW_MOVIE_OVERRIDE=<name>[,<name>...] (debug) plays those movies (e.g. Semo02, BONUS03) instead of the
        // ones the game loads, one per load: the first replaces the intro, the next ones are started from the title menu by
        // gowDiagBonusMovie (tools/movies.py plays every movie of the disc in one session this way). 0x188CA0 copies its
        // name argument (a0) into the movie struct (0x304D10), so the override name is put on the guest stack below sp and
        // a0 points there; the game's own buffer (the intro's holds 6 characters) is not written.
        std::vector<std::string> &movieOverrideList()
        {
            static std::vector<std::string> list = [] {
                std::vector<std::string> names;
                if (const char *value = std::getenv("GOW_MOVIE_OVERRIDE"))
                {
                    std::string text(value);
                    for (size_t start = 0; start <= text.size();)
                    {
                        const size_t end = std::min(text.find(',', start), text.size());
                        if (end > start && end - start <= 31)
                            names.push_back(text.substr(start, end - start));
                        start = end + 1;
                    }
                }
                return names;
            }();
            return list;
        }
        size_t g_movieOverrideNext = 0;

        // GOW-Port: the movie player's video ring (player struct *0x304F20: +0xE8 base, +0xEC size, +0xF0 write, +0xF4/+0xF8
        // the block sent to the IPU) treats a write pointer at the ring's end as data: once the IPU block pointer wraps to
        // the base, "available = write - block end" is the whole ring, so the no-data callback (0x18AAA8) keeps sending
        // the old ring and never asks for more data, and the movie stops while its sound plays on (P04MP05 after 203
        // pictures: the demuxed video filled the ring exactly to its end). The end and the base are the same position;
        // the write pointer is moved to the base before the readers look at it: at the start of the no-data callback
        // (where "available" is computed) and around the writer (stream callback 0x18A788). Only code before a guest call
        // is reliable in a wrapper: a call can return early at an EE checkpoint and be finished later through the
        // original's own slots.
        void gowNormaliseVideoRing(uint8_t *rdram)
        {
            if (const uint32_t movie = readGuest32(rdram, 0x00304F20u); movie != 0u)
            {
                const uint32_t base = readGuest32(rdram, movie + 0xE8u), size = readGuest32(rdram, movie + 0xECu);
                if (size != 0u && readGuest32(rdram, movie + 0xF0u) == base + size)
                    writeGuest32(rdram, movie + 0xF0u, base);
            }
        }
    }

    // Optional FMV bypass at the movie API, before buffers/RPCs are allocated.
    // Keep the normal MPEG path available for decoder investigation.
    void gowSkipMovieLoad(uint8_t *rdram, R5900Context *ctx, PS2Runtime *)
    {
        const auto *name = guestBuffer(rdram, GPR_U32(ctx, 4), 128);
        std::fprintf(stderr, "[gow-fmv] skipped %.127s\n", name ? reinterpret_cast<const char *>(name) : "<invalid>");
        writeGuest32(rdram, 0x0029C838u, 0u);
        writeGuest32(rdram, 0x0029C870u, 0u);
        writeGuest32(rdram, 0x0029C840u, 0u);
        returnToCaller(ctx, 0u);
    }

    size_t movieOverrideCount()
    {
        return movieOverrideList().size();
    }

    void gowMovieOverrideLoad(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const std::vector<std::string> &list = movieOverrideList();
        if (g_movieOverrideNext >= list.size())
        {
            // GOW_MPEG_DIAG: the movies the game loads itself (tools/movies.py --new-game tells them apart by this line)
            if (const uint8_t *name = guestBuffer(rdram, GPR_U32(ctx, 4), 1))
                std::fprintf(stderr, "[gow-fmv] load %.31s\n", reinterpret_cast<const char *>(name));
            sub_00188CA0_0x188ca0(rdram, ctx, runtime);
            return;
        }
        const std::string &override = list[g_movieOverrideNext++];
        const uint32_t sp = GPR_U32(ctx, 29);
        const uint32_t name = sp - 32u;
        uint8_t *buffer = guestBuffer(rdram, name, 32);
        const uint8_t *old = guestBuffer(rdram, GPR_U32(ctx, 4), 1);
        std::fprintf(stderr, "[gow-fmv] override %zu/%zu %s -> %s\n", g_movieOverrideNext, list.size(),
                     old ? reinterpret_cast<const char *>(old) : "?", override.c_str());
        std::memset(buffer, 0, 32);
        std::memcpy(buffer, override.data(), override.size());
        SET_GPR_U32(ctx, 4, name);
        SET_GPR_U32(ctx, 29, name);
        sub_00188CA0_0x188ca0(rdram, ctx, runtime);
        SET_GPR_U32(ctx, 29, sp);
    }

    // GOW-Port: GOW_BONUS_MOVIE=<NN> (debug) plays bonus movie NN (BONUS03, 06, 10, 12, 14, 16, 17, 18, 24, 25) from
    // the main menu the way the Treasures menu does, without unlocking it: its command handler 0x1BA2C8 writes the two
    // digits into the game's "BonusXX" buffer (0x29CC00) and sets the menu object's movie name (+0x1518) and +0x1520;
    // the menu update 0x1BA4F8 then starts it with 0x1B2D20(name, 3, 1) and returns to the menu when it ends. The
    // request is made once, GOW_BONUS_MOVIE_DELAY seconds (default 10) after the menu update first runs. With a
    // GOW_MOVIE_OVERRIDE list, a request is made each time the movie thread is idle again (3 s after the previous movie
    // ended) while list entries remain; the load override then plays the next entry instead of the bonus movie.
    void gowDiagBonusMovie(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static const char *movie = [] { const char *v = std::getenv("GOW_BONUS_MOVIE"); return v ? v : "24"; }();
        static const double delay = [] { const char *v = std::getenv("GOW_BONUS_MOVIE_DELAY"); return v ? std::atof(v) : 10.0; }();
        static auto readyAt = std::chrono::steady_clock::now() + std::chrono::duration_cast<std::chrono::steady_clock::duration>(
                                                                        std::chrono::duration<double>(delay));
        static bool done = false, inFlight = false;
        const bool listMode = movieOverrideList().size() > 1;
        const uint32_t movieState = readGuest32(rdram, 0x0029C838u);
        const bool wanted = listMode ? g_movieOverrideNext < movieOverrideList().size() : !done;
        if (wanted && !inFlight && movieState == 0u && std::chrono::steady_clock::now() >= readyAt)
        {
            done = true;
            uint8_t *name = guestBuffer(rdram, 0x0029CC00u, 8);
            uint8_t *menu = guestBuffer(rdram, GPR_U32(ctx, 4), 0x1528);
            if (name && menu && std::strlen(movie) == 2 && std::memcmp(name, "Bonus", 5) == 0)
            {
                name[5] = static_cast<uint8_t>(movie[0]);
                name[6] = static_cast<uint8_t>(movie[1]);
                const uint32_t address = 0x0029CC00u, one = 1u;
                std::memcpy(menu + 0x1518, &address, 4);
                std::memcpy(menu + 0x1520, &one, 4);
                inFlight = true;
                std::fprintf(stderr, "[gow-fmv] bonus request %.7s (menu %08x)\n", reinterpret_cast<char *>(name),
                             GPR_U32(ctx, 4));
            }
        }
        // movie thread state (0x29C838: 10 loaded, 11 playing, 0 idle) as seen by the menu, for the test scripts
        static uint32_t lastState = 0u;
        if (done && movieState != lastState)
        {
            std::fprintf(stderr, "[gow-fmv] menu sees movie state %u -> %u\n", lastState, movieState);
            if (movieState == 0u && inFlight)
            {
                inFlight = false;
                readyAt = std::chrono::steady_clock::now() + std::chrono::seconds(3);
            }
            lastState = movieState;
        }
        sub_001BA4F8_0x1ba4f8(rdram, ctx, runtime);
    }

    // GOW_MPEG_DIAG (debug): the movie's stream reads (0x17AFE0(stream, buffer, size, ...)): size asked, file size and
    // position, and the size returned, for reads close to the end of the file
    void gowDiagStreamRead(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t stream = GPR_U32(ctx, 4), asked = GPR_U32(ctx, 6);
        const uint32_t size = readGuest32(rdram, stream + 8u), position = readGuest32(rdram, stream + 0xCu);
        sub_0017AFE0_0x17afe0(rdram, ctx, runtime);
        // near the end of a file, or every read of a big stream (a movie) with GOW_MPEG_DIAG=2
        static const bool allReads = [] { const char *v = std::getenv("GOW_MPEG_DIAG"); return v && v[0] == '2'; }();
        if (size - position < 0x10000u || (allReads && size > 0x100000u))
            std::fprintf(stderr, "[gow-fmv-read] stream=%x asked=%x size=%x position=%x -> %x flags=%x\n", stream, asked, size,
                         position, GPR_U32(ctx, 2), readGuest32(rdram, stream + 4u));
    }

    void gowMovieVideoRingWrite(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc == 0x0018A788u) // not a resume inside the original
            gowNormaliseVideoRing(rdram);
        sub_0018A788_0x18a788(rdram, ctx, runtime);
    }

    void gowMovieNodata(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc == 0x0018AAA8u)
            gowNormaliseVideoRing(rdram);
        sub_0018AAA8_0x18aaa8(rdram, ctx, runtime);
    }

    // GOW-Port: end of a movie's file (P04MP05, the Cliffs of Madness lever movie, froze after its last picture).
    // The game reads a movie through a stream (0x2FDE10: +0x28 state, 3 = the backing file reached its end, +0x24 the
    // 16 KB blocks still queued) and the read callback 0x188848 hands out the queued blocks. Once they are gone it
    // returns 0, and the refill 0x18AC20 takes 0 for "nothing yet": it calls the callback again, for ever, in its
    // blocking path. Only a negative result ends the movie (the refill stores 1 at movie+0x154, the read-EOF flag).
    // The callback answers -1 instead of 0 when the refill asks for data and the stream is at its end with nothing
    // queued. Everything else (data queued, the cancel flag 0x29C848, other callers, resumptions) is the original.
    //
    // GOW_MOVIE_EOF=0 turns both wrappers of the end of a movie off (the original behaviour, to compare).
    namespace
    {
        constexpr uint32_t kMoviePlayer = 0x00304F20u, kMovieStream = 0x002FDE10u, kMovieCancel = 0x0029C848u;

        bool movieEofFixEnabled()
        {
            static const bool on = [] { const char *v = std::getenv("GOW_MOVIE_EOF"); return !v || v[0] != '0'; }();
            return on;
        }

        // the stream is at the end of its file and every block has been handed out
        bool movieStreamDrained(const uint8_t *rdram)
        {
            return readGuest32(rdram, kMovieStream + 0x28u) == 3u && readGuest32(rdram, kMovieStream + 0x24u) == 0u &&
                   readGuest32(rdram, kMovieCancel) == 0u;
        }
    }

    void gowMovieStreamRead(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        // a1 = size asked (positive), ra = the refill's two call sites (0x18AC70: first read, 0x18AD34: blocking retry)
        const uint32_t ra = GPR_U32(ctx, 31);
        if (ctx->pc == 0x00188848u && movieEofFixEnabled() && (ra == 0x0018AC70u || ra == 0x0018AD34u) &&
            static_cast<int32_t>(GPR_U32(ctx, 5)) > 0 && movieStreamDrained(rdram))
        {
            static bool logged = false;
            if (!logged)
            {
                logged = true;
                std::fprintf(stderr, "[gow-fmv] stream drained: the read callback reports the end of the file\n");
            }
            returnToCaller(ctx, 0xffffffffu);
            return;
        }
        sub_00188848_0x188848(rdram, ctx, runtime);
    }

    // GOW-Port: the movie's completion test 0x189B00 returns 1 as soon as the read-EOF flag (movie+0x154) is set, but
    // the decoder can still hold pictures, so the last ones of a movie would be cut. For an MPEG movie (+0x140 = 2) whose stream is drained, the test waits
    // until the decoder's reference buffer is empty, as it does without the flag: sceMpegIsRefBuffEmpty (0x26FF70) is
    // two loads, *(*(movie + 0x40)). A movie that never empties its buffer is let go after GOW_MOVIE_EOF_WAIT_MS
    // (default 1500) of host time, so a decoder that stopped can never keep the player here.
    void gowMovieCompletion(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static std::chrono::steady_clock::time_point waitingSince{};
        static bool waiting = false;
        if (ctx->pc == 0x00189B00u && movieEofFixEnabled())
        {
            const uint32_t movie = readGuest32(rdram, kMoviePlayer);
            if (movie != 0u && readGuest32(rdram, movie + 0x154u) != 0u && readGuest32(rdram, movie + 0x140u) == 2u &&
                movieStreamDrained(rdram))
            {
                const uint32_t refBuffer = readGuest32(rdram, movie + 0x40u);
                const bool pending = guestBuffer(rdram, refBuffer, 4) != nullptr && readGuest32(rdram, refBuffer) == 0u;
                static const long limitMs = [] {
                    const char *v = std::getenv("GOW_MOVIE_EOF_WAIT_MS");
                    return v ? std::max(0l, std::atol(v)) : 1500l;
                }();
                if (pending)
                {
                    const auto now = std::chrono::steady_clock::now();
                    if (!waiting)
                    {
                        waiting = true;
                        waitingSince = now;
                        std::fprintf(stderr, "[gow-fmv] end of stream: waiting for the decoder to show its last pictures\n");
                    }
                    if (std::chrono::duration_cast<std::chrono::milliseconds>(now - waitingSince).count() < limitMs)
                    {
                        returnToCaller(ctx, 0u);
                        return;
                    }
                    std::fprintf(stderr, "[gow-fmv] decoder still holds pictures after %ld ms, ending the movie\n", limitMs);
                }
            }
            waiting = false;
        }
        sub_00189B00_0x189b00(rdram, ctx, runtime);
    }

    // GOW-Port: the movie thread never blocks: it polls its stream service (0x188B80, called only by the movie
    // player 0x188848) as fast as it can. Guest time only advances by executed cycles, and the recompiled poll costs
    // more host time per guest cycle than a real EE, so guest time, the IOP and the movie sound fell to ~0.72x real
    // time (pictures at ~22 fps instead of 30). Each poll counts the host time that has passed as guest time.
    void gowMovieStreamService(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        // GOW_MPEG_DIAG: once per second, the movie struct words (0x304D10 + 0x80..0x140) that changed, the movie
        // thread state and the poll's caller (what a stuck player waits for)
        static const bool diag = std::getenv("GOW_MPEG_DIAG") != nullptr;
        if (diag)
        {
            static auto last = std::chrono::steady_clock::time_point{};
            static uint32_t previous[0x30] = {};
            static uint64_t polls = 0;
            ++polls;
            if (std::chrono::steady_clock::now() - last > std::chrono::seconds(1))
            {
                last = std::chrono::steady_clock::now();
                std::string changed;
                for (uint32_t i = 0; i < 0x30u; ++i)
                {
                    const uint32_t value = readGuest32(rdram, 0x00304D90u + i * 4u);
                    if (value != previous[i])
                    {
                        char text[32];
                        std::snprintf(text, sizeof(text), " +%02x=%x", 0x80u + i * 4u, value);
                        changed += text;
                        previous[i] = value;
                    }
                }
                std::fprintf(stderr, "[gow-fmv-poll] polls=%llu state=%u ra=%x%s\n", static_cast<unsigned long long>(polls),
                             readGuest32(rdram, 0x0029C838u), GPR_U32(ctx, 31), changed.c_str());
            }
        }
        runtime->eeScheduler().catchUpToHost(EeScheduler::kEeClockHz / 60u);
        sub_00188B80_0x188b80(rdram, ctx, runtime);
    }

    // GOW-Port: the end of a movie. After the last picture the player drains its display queue in a loop at 0x189794: it
    // refills the audio (0x189A88, return address 0x18979C), polls 0x26CE98 and repeats until the VBlank handler has emptied
    // the queue. That loop never calls the stream service, so the host-time catch-up above does not apply to it, and the
    // last fields of the movie's sound reach the mixer as guest time creeps forward cycle by cycle: the device runs dry
    // right before the transition. The same one-field catch-up is applied at that call site only.
    void gowMovieTailAudio(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc == 0x00189A88u && GPR_U32(ctx, 31) == 0x0018979Cu)
            runtime->eeScheduler().catchUpToHost(EeScheduler::kEeClockHz / 60u);
        sub_00189A88_0x189a88(rdram, ctx, runtime);
    }

    void gowSkipMovieReady(uint8_t *, R5900Context *ctx, PS2Runtime *) { returnToCaller(ctx, 1u); }
    void gowSkipMovieNoop(uint8_t *, R5900Context *ctx, PS2Runtime *) { returnToCaller(ctx, 0u); }

    // The level script's movie start request (0x188E80): logged so skipped runs show when a movie would play.
    void gowSkipMovieStart(uint8_t *, R5900Context *ctx, PS2Runtime *)
    {
        std::fprintf(stderr, "[gow-fmv] start request skipped (ra=%x)\n", GPR_U32(ctx, 31));
        returnToCaller(ctx, 0u);
    }
}
