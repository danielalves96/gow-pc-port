#ifndef PS2_PERF_H
#define PS2_PERF_H

// GOW-Port: optional exclusive elapsed-time profile, split by thread.
// The times include system waits; they do not represent CPU usage samples.
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cstring>
#include <string>
#include <algorithm>
#if defined(__x86_64__) || defined(_M_X64)
#if defined(_MSC_VER)
#include <intrin.h>
#else
#include <x86intrin.h>
#endif
#endif

namespace ps2_perf
{
    enum class Bucket : size_t { Ee, Vu, Gs, Iop, GuestWait, Upload, HostWait, Count, None = Count };
    constexpr size_t bucketCount = static_cast<size_t>(Bucket::Count);

    // Accounting with an injectable clock to check nesting without sleeps.
    struct Accounting
    {
        Bucket active = Bucket::None;
        uint64_t since = 0;

        struct Charge { Bucket bucket; uint64_t nanos; };
        Charge switchTo(Bucket next, uint64_t now)
        {
            const Charge charge{active, now >= since ? now - since : 0u};
            active = next;
            since = now;
            return charge;
        }
    };

    inline bool enabled()
    {
        static const bool on = [] {
            const char *value = std::getenv("GOW_PERF_DIAG");
            return value && std::strcmp(value, "1") == 0;
        }();
        return on;
    }
    inline bool countFrames()
    {
        static const bool on = [] {
            const char *value = std::getenv("GOW_PERF_DIAG");
            return value && (std::strcmp(value, "1") == 0 || std::strcmp(value, "frames") == 0);
        }();
        return on;
    }
    inline std::array<std::atomic<uint64_t>, bucketCount> nanos{};
    inline std::atomic<uint64_t> guestFlips{0}, hostFrames{0};
    inline thread_local Accounting accounting;
    inline uint64_t steadyNanos()
    {
        return static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count());
    }
#if defined(__x86_64__) || defined(_M_X64)
    // GOW-Port: the buckets are read from the time-stamp counter, scaled once against steady_clock. The launcher always
    // sets GOW_PERF_DIAG=1, and every Scope read the clock twice (QueryPerformanceCounter on Windows): hundreds of
    // thousands of reads a second across the EE, VU1 and GS threads, ~7 % of the game thread on 2 cores. The buckets are
    // diagnostics only ([gow-perf], the hitch report), so the TSC's precision is plenty; on an old CPU without an
    // invariant TSC they would drift with the clock speed, and nothing else depends on them.
    inline double tscNanosPerTick()
    {
        static const double scale = [] {
            const uint64_t t0 = steadyNanos();
            const uint64_t c0 = __rdtsc();
            uint64_t t1 = t0;
            while (t1 - t0 < 2000000u) // 2 ms, once per process
                t1 = steadyNanos();
            const uint64_t c1 = __rdtsc();
            return c1 > c0 ? static_cast<double>(t1 - t0) / static_cast<double>(c1 - c0) : 1.0;
        }();
        return scale;
    }
    inline uint64_t nowNanos()
    {
        return static_cast<uint64_t>(static_cast<double>(__rdtsc()) * tscNanosPerTick());
    }
#else
    inline uint64_t nowNanos() { return steadyNanos(); }
#endif
    inline void switchTo(Bucket next)
    {
        const auto charge = accounting.switchTo(next, nowNanos());
        if (charge.bucket != Bucket::None)
            nanos[static_cast<size_t>(charge.bucket)].fetch_add(charge.nanos, std::memory_order_relaxed);
    }
    class Scope
    {
        bool on;
        Bucket previous = Bucket::None;
    public:
        explicit Scope(Bucket bucket) : on(enabled())
        {
            if (on) { previous = accounting.active; switchTo(bucket); }
        }
        ~Scope() { if (on) switchTo(previous); }
        Scope(const Scope &) = delete;
        Scope &operator=(const Scope &) = delete;
    };
    // GOW-Port: GOW_DELTA_DIAG=1 = game-speed meter (idea from KIexster's ps2recomp-game-speed.patch).
    // On every entry to GOW_PERF_FRAME_PC (vid::Flip, once per game frame) it adds the game's own frame
    // step (float at 0x29C64C in SCUS-97399, seconds) and once per wall second prints
    // [gow-speed] ... speed=<game seconds / wall seconds>: 1.00 = real time, >1 fast, <1 slow motion.
    // Guest code runs one function at a time, so the plain statics need no lock.
    inline bool deltaDiag()
    {
        static const bool on = [] {
            const char *value = std::getenv("GOW_DELTA_DIAG");
            return value && std::strcmp(value, "1") == 0;
        }();
        return on;
    }
    inline void gameSpeedFrame(const uint8_t *rdram)
    {
        static uint64_t windowStart = nowNanos(), frames = 0;
        static double gameSeconds = 0.0, minDelta = 1e9, maxDelta = 0.0;
        float delta = 0.0f;
        std::memcpy(&delta, rdram + 0x29C64Cu, sizeof(delta));
        if (delta > 0.0f && delta < 1.0f)
        {
            gameSeconds += delta;
            if (delta < minDelta) minDelta = delta;
            if (delta > maxDelta) maxDelta = delta;
        }
        // T0 ticks the game measured since its previous frame (0x29C670), in tenths of an NTSC field (9610 ticks).
        static uint32_t hist[24] = {};
        int32_t ticks = 0;
        std::memcpy(&ticks, rdram + 0x29C670u, sizeof(ticks));
        ++hist[std::min<uint32_t>(23u, static_cast<uint32_t>(std::max(0, ticks)) / 961u)];
        ++frames;
        const uint64_t now = nowNanos();
        const double wall = static_cast<double>(now - windowStart) * 1e-9;
        if (wall < 1.0) return;
        std::string line;
        for (uint32_t i = 0; i < 24u; ++i)
            if (hist[i]) line += " " + std::to_string(i) + ":" + std::to_string(hist[i]), hist[i] = 0;
        std::fprintf(stderr, "[gow-speed-hist] field/10 counts:%s\n", line.c_str());
        std::fprintf(stderr, "[gow-speed] frames=%llu wall=%.2f game=%.3f speed=%.3f delta_min=%.4f delta_max=%.4f\n",
                     static_cast<unsigned long long>(frames), wall, gameSeconds, gameSeconds / wall,
                     frames && maxDelta > 0.0 ? minDelta : 0.0, maxDelta);
        windowStart = now; frames = 0; gameSeconds = 0.0; minDelta = 1e9; maxDelta = 0.0;
    }
    inline void guestEntry(uint32_t pc, const uint8_t *rdram = nullptr)
    {
        static const bool frames = countFrames(), speed = deltaDiag();
        if (!frames && !speed) return;
        static const uint32_t framePc = [] {
            const char *value = std::getenv("GOW_PERF_FRAME_PC");
            return value ? static_cast<uint32_t>(std::strtoul(value, nullptr, 0)) : 0u;
        }();
        // Only entries to the chosen function: resumptions do not count as new frames.
        if (framePc == 0u || pc != framePc) return;
        if (frames) guestFlips.fetch_add(1, std::memory_order_relaxed);
        if (speed && rdram) gameSpeedFrame(rdram);
    }
    inline void report()
    {
        if (!countFrames()) return;
        // Called only from the presentation thread. It does not read game RAM.
        const uint64_t now = nowNanos();
        static uint64_t first = now, last = now;
        static std::array<uint64_t, bucketCount> before{};
        static uint64_t previousFlips = 0, previousFrames = 0;
        if (now - last < 5000000000ull) return;
        const double seconds = static_cast<double>(now - last) * 1e-9;
        std::array<double, bucketCount> ms{};
        for (size_t i = 0; i < bucketCount; ++i)
        {
            const uint64_t total = nanos[i].load(std::memory_order_relaxed);
            ms[i] = static_cast<double>(total - before[i]) * 1e-6;
            before[i] = total;
        }
        const uint64_t flips = guestFlips.load(std::memory_order_relaxed);
        const uint64_t frames = hostFrames.load(std::memory_order_relaxed);
        std::fprintf(stderr, "[gow-perf] t=%.2f window=%.2f ee_ms=%.2f vu_ms=%.2f gs_ms=%.2f iop_ms=%.2f guest_wait_ms=%.2f upload_ms=%.2f host_wait_ms=%.2f guest_flip_hz=%.2f host_hz=%.2f\n",
            (now - first) * 1e-9, seconds, ms[0], ms[1], ms[2], ms[3], ms[4], ms[5], ms[6],
            (flips - previousFlips) / seconds, (frames - previousFrames) / seconds);
        previousFlips = flips; previousFrames = frames; last = now;
    }
}
#endif
