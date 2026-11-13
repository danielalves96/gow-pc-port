// GOW-Port: game logic rate (high logic FPS, logs/plan/PLANO-FPS-ALTO-0.3.1.pt-BR.md). Experimental and off by
// default: with PC_LOGIC_FPS absent or 60 only the optional diagnostic (GOW_HFPS_DIAG=1) is bound.
//
// sub_0017D778 (0x17D778) runs once per game frame. It reads EE timer 0 (BUSCLK/256 = 576 kHz), keeps a 64-bit
// tick total and computes the frame's time step:
//   0x29C670  ticks since the previous call
//   0x29C654  floor in NTSC fields, 60 / [0x29C640] (int; [0x29C640] = 60 or 30, the 30 FPS mode)
//   0x29C650  that floor in seconds (float; 0x14A560 turns it back into frames per second)
//   fields  = clamp(ticks / 9610, 1, 4) (11520 ticks in PAL, [0x331078] == 3), then max(fields, floor)
//   fixed   = the floor alone, when [0x29C65C] == 0 or the one-shot flag [0x29C660] is set (cleared here)
//   0x29C64C  delta = fields / 60 (float seconds)
//   0x29C658  delta * [0x29C644] * [0x29C648] (float; 0x29C648 is the slow-motion factor written by 0x203F08)
//
// The frame end 0x17A2A0 then waits in 0x17A1D0(a0 = [0x29C654]) until the game's VBlank handler (0x182F28) has
// counted a0 VBlanks in 0x29C7D4, and clears that counter. So the game never runs faster than 60 frames per second
// and never steps less than 1/60 s.
//
// With PC_LOGIC_FPS=N (N = 120, the only rate above 60: the project caps the game at 120 FPS) the runtime's VBlank stays at 60 Hz (it also counts video fields, the movie
// sync and other waits) and two things change:
//   - gowLogicFrameWait (0x17A1D0, only the call from the frame end): sleeps the EE thread to the next 1/N s slot of
//     guest time instead of counting VBlanks;
//   - gowLogicTimeStep (0x17D778): after the original, on the measured path only, rewrites the delta with a floor of
//     60/N fields instead of 1. 0x29C650/0x29C654 keep their 60 Hz values.
// Frames that use the fixed step, the game's own 30 FPS mode and movies (video thread state 0x29C838 3..11) fall back
// to the original 60 Hz wait.

#include "gow_game.h"
#include "gow_guest_functions.h"
#include "runtime/ee_scheduler.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace gow_game
{
    namespace
    {
        constexpr uint32_t kTimeStepEntry = 0x0017D778u;
        constexpr uint32_t kFrameWaitEntry = 0x0017A1D0u;
        constexpr uint32_t kFrameEndReturn = 0x0017A36Cu; // 0x17A2A0's call of 0x17A1D0 returns here
        constexpr uint32_t kTicks = 0x0029C670u;
        constexpr uint32_t kDelta = 0x0029C64Cu;
        constexpr uint32_t kScaledDelta = 0x0029C658u;
        constexpr uint32_t kFloorFields = 0x0029C654u;
        constexpr uint32_t kFloorSeconds = 0x0029C650u;
        constexpr uint32_t kFrameRate = 0x0029C640u;
        constexpr uint32_t kScaleA = 0x0029C644u;
        constexpr uint32_t kScaleB = 0x0029C648u;
        constexpr uint32_t kMeasuredEnabled = 0x0029C65Cu;
        constexpr uint32_t kFixedOnce = 0x0029C660u;
        constexpr uint32_t kVBlankCount = 0x0029C7D4u;
        constexpr uint32_t kVideoMode = 0x00331078u; // 3: PAL timing for the tick-to-field ratio
        constexpr uint32_t kMovieState = 0x0029C838u; // video thread: 0 idle, 3..9 loading, 10 loaded, 11 playing

        float readFloat(const uint8_t *rdram, uint32_t address)
        {
            const uint32_t bits = readGuest32(rdram, address);
            float value;
            std::memcpy(&value, &bits, sizeof(value));
            return value;
        }

        void writeFloat(uint8_t *rdram, uint32_t address, float value)
        {
            uint32_t bits;
            std::memcpy(&bits, &value, sizeof(bits));
            writeGuest32(rdram, address, bits);
        }

        bool diagEnabled()
        {
            static const bool on = [] { const char *v = std::getenv("GOW_HFPS_DIAG"); return v && std::strcmp(v, "1") == 0; }();
            return on;
        }

        // A movie is loading or playing (the player struct *0x304F20 stays allocated afterwards, so it is not a test).
        bool moviePlaying(const uint8_t *rdram)
        {
            const uint32_t state = readGuest32(rdram, kMovieState);
            return state >= 3u && state <= 11u;
        }

        // The game frame takes the 60 Hz path: fixed step (now or on the next step), the game's 30 FPS mode, a movie.
        bool fallsBackTo60(const uint8_t *rdram)
        {
            return readGuest32(rdram, kMeasuredEnabled) == 0u || readGuest32(rdram, kFixedOnce) != 0u ||
                   readGuest32(rdram, kFloorFields) != 1u || moviePlaying(rdram);
        }

        struct Window
        {
            std::chrono::steady_clock::time_point start = std::chrono::steady_clock::now();
            unsigned calls = 0, fixedDisabled = 0, fixedOnce = 0, atFloor = 0, atCeiling = 0;
            unsigned waits = 0, lateFrames = 0, fallbackWaits = 0;
            double ticks = 0.0, fields = 0.0, delta = 0.0, scaled = 0.0;
            uint32_t minTicks = ~0u, maxTicks = 0u;
            float minDelta = 1e9f, maxDelta = 0.0f;
        };
        Window g_window;

        struct Scales
        {
            bool known = false;
            float a = 0.0f, b = 0.0f;
            uint32_t rate = 0u;
        };

        void logScalesIfChanged(const uint8_t *rdram, Scales &last, unsigned call)
        {
            const float a = readFloat(rdram, kScaleA);
            const float b = readFloat(rdram, kScaleB);
            const uint32_t rate = readGuest32(rdram, kFrameRate);
            if (last.known && a == last.a && b == last.b && rate == last.rate)
                return;
            std::fprintf(stderr, "[gow-hfps] call=%u scales %.4f x %.4f (was %.4f x %.4f) rate=%u floor=%d (%.5f s)\n", call,
                         a, b, last.a, last.b, rate, static_cast<int32_t>(readGuest32(rdram, kFloorFields)),
                         readFloat(rdram, kFloorSeconds));
            last = {true, a, b, rate};
        }

        void flushWindow(Window &w, double seconds)
        {
            if (w.calls == 0u)
                return;
            const double n = w.calls;
            std::fprintf(stderr,
                         "[gow-hfps] %.1fs logic=%d calls=%u (%.1f/s) ticks avg=%.0f min=%u max=%u fields avg=%.3f "
                         "floor=%u ceil=%u delta avg=%.5f min=%.5f max=%.5f scaled avg=%.5f fixed=%u (disabled=%u "
                         "once=%u) waits=%u late=%u at60=%u game_s/host_s=%.3f\n",
                         seconds, gowLogicFps(), w.calls, n / seconds, w.ticks / n, w.minTicks, w.maxTicks,
                         w.fields / n, w.atFloor, w.atCeiling, w.delta / n, w.minDelta, w.maxDelta, w.scaled / n,
                         w.fixedDisabled + w.fixedOnce, w.fixedDisabled, w.fixedOnce, w.waits, w.lateFrames,
                         w.fallbackWaits, w.delta / seconds);
            w = Window{};
        }

        void record(const uint8_t *rdram, bool measured, bool once, float floorFields)
        {
            static Scales scales;
            static unsigned total = 0;
            ++total;
            logScalesIfChanged(rdram, scales, total);

            Window &w = g_window;
            const uint32_t ticks = readGuest32(rdram, kTicks);
            const float delta = readFloat(rdram, kDelta);
            const float fields = delta * 60.0f;
            w.calls++;
            w.ticks += ticks;
            w.minTicks = std::min(w.minTicks, ticks);
            w.maxTicks = std::max(w.maxTicks, ticks);
            w.fields += fields;
            w.delta += delta;
            w.scaled += readFloat(rdram, kScaledDelta);
            w.minDelta = std::min(w.minDelta, delta);
            w.maxDelta = std::max(w.maxDelta, delta);
            if (!measured)
                w.fixedDisabled++;
            else if (once)
                w.fixedOnce++;
            else if (fields <= floorFields + 1e-4f)
                w.atFloor++;
            else if (fields >= 4.0f - 1e-4f)
                w.atCeiling++;

            const double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now() - w.start).count();
            if (seconds >= 5.0)
                flushWindow(w, seconds);
        }

        // The measured delta with a floor of 60/N fields: the original's float math with the lower clamp moved.
        void rewriteMeasuredDelta(uint8_t *rdram, int logicFps)
        {
            const uint32_t ticks = readGuest32(rdram, kTicks);
            const float perField = readGuest32(rdram, kVideoMode) == 3u ? 115.2f : 96.1f; // ticks / 100 per field
            const float lower = 6000.0f / static_cast<float>(logicFps);                   // 100 * 60/N
            const float hundredths = std::min(std::max(static_cast<float>(ticks) / perField, lower), 400.0f);
            const float delta = (hundredths / 100.0f) / 60.0f;
            writeFloat(rdram, kDelta, delta);
            writeFloat(rdram, kScaledDelta, delta * readFloat(rdram, kScaleA) * readFloat(rdram, kScaleB));
        }
    }

    // GOW-Port: 0x17D778, bound with PC_LOGIC_FPS > 60 or GOW_HFPS_DIAG=1. Always runs the original first.
    void gowLogicTimeStep(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != kTimeStepEntry) // resumption after an EE checkpoint inside the original
        {
            sub_0017D778_0x17d778(rdram, ctx, runtime);
            return;
        }
        const bool measured = readGuest32(rdram, kMeasuredEnabled) != 0u;
        const bool once = readGuest32(rdram, kFixedOnce) != 0u;
        const uint32_t ra = GPR_U32(ctx, 31);
        sub_0017D778_0x17d778(rdram, ctx, runtime);
        if (ctx->pc != ra) // suspended at a checkpoint: the globals are not final yet
            return;
        const int logicFps = gowLogicFps();
        float floorFields = static_cast<float>(static_cast<int32_t>(readGuest32(rdram, kFloorFields)));
        if (logicFps > 60 && measured && !once && readGuest32(rdram, kFloorFields) == 1u && !moviePlaying(rdram))
        {
            rewriteMeasuredDelta(rdram, logicFps);
            floorFields = 60.0f / static_cast<float>(logicFps);
        }
        if (diagEnabled())
            record(rdram, measured, once, floorFields);
    }

    // GOW-Port: 0x17A1D0 (wait for a0 VBlanks, then clear the counter), bound with PC_LOGIC_FPS > 60. Only the frame
    // end's call is paced at N Hz; the movie player's calls, the fallback frames and resumptions run the original.
    void gowLogicFrameWait(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        static uint64_t nextSlot = 0u; // guest cycle of the next logic frame
        const uint32_t ra = GPR_U32(ctx, 31);
        const bool interruptsOn = (ctx->cop0_status & 0x10000u) != 0u; // the thread may block (as in DelayThread)
        if (ctx->pc != kFrameWaitEntry || ra != kFrameEndReturn || !interruptsOn || gowLogicFps() <= 60 || fallsBackTo60(rdram))
        {
            if (ctx->pc == kFrameWaitEntry && ra == kFrameEndReturn)
            {
                if (nextSlot != 0u) // leaving the N Hz pacing: the counter ran during it, start a clean VBlank wait
                    writeGuest32(rdram, kVBlankCount, 0u);
                nextSlot = 0u;
                if (diagEnabled())
                    g_window.fallbackWaits++;
            }
            sub_0017A1D0_0x17a1d0(rdram, ctx, runtime);
            return;
        }
        const uint64_t period = EeScheduler::kEeClockHz / static_cast<uint64_t>(gowLogicFps());
        const uint64_t now = runtime->eeCheckpointClock().eeCycle;
        // The original clears the counter after its wait; nothing reads it but the VBlank waits, which count from 0.
        writeGuest32(rdram, kVBlankCount, 0u);
        ctx->pc = ra;
        // A strict limit: a late frame runs at once and the next slot counts from now, so frames never catch up above N.
        if (nextSlot == 0u || now >= nextSlot)
        {
            nextSlot = now + period;
            if (diagEnabled())
                g_window.lateFrames++;
            return;
        }
        const uint64_t wait = nextSlot - now;
        nextSlot += period;
        if (diagEnabled())
            g_window.waits++;
        runtime->eeScheduler().delayCurrent(wait); // resumes at $ra
    }
}
