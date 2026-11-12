// GOW-Port: PC Port functions over the Waffle565 runtime (God-Of-War-Native-Port).
//
// The Waffle runtime (native GS via render targets, compiled IOP and VU1, native videos) is the base. Here, with
// the fewest possible changes to its files, the PC port's functions are wired in:
//   - Launcher window: PC_WIDTH / PC_HEIGHT (or GOW_WINDOW=<w>x<h>), PC_SCREEN=completa
//     (F11 toggles it), PC_VSYNC=1, PC_TITLE, PC_ICON, PC_ESC_QUITS=0, PC_WINDOW_FPS.
//   - Image aspect ratio: PC_ASPECT ("stretch", "4:3", "21:9", "32:9"...) and the widescreen switch, whose aspect comes in
//     PC_WIDESCREEN (4:3 < aspect <= 4.0, so up to 32:9; 21:9 and 32:9 are experimental); with neither, the original 4:3
//     picture.
//   - Widescreen Hor+ (PC_WIDESCREEN=A, the launcher's "Widescreen" switch): the projection reads the factor from 0x7F00
//     (patch in games/scus-97399/gow-port.toml) and it is written here every VBlank. With the switch off the game stays
//     in its original 4:3 mode (gow_pad2.cpp does not force its own widescreen) and F = 0.75, so F * aspect is exactly 1:
//     the image is bit-for-bit the same as without the patch.
//   - Configurable keyboard (PC_KEYS, ps2_pad.cpp), cheat menu (F2, gow_cheats.cpp), target lock-on
//     (Alt, gow_targeting.cpp), agile combat (gow_animation_cancel.cpp), progressive scan (gow_video.cpp) and the function probe (GOW_POLLING, gow_polling.cpp).
//   - gow_wait.cpp (the EE sleeps while waiting for the frame) is optional with GOW_EE_WAIT=1: Waffle measures the
//     game's speed with that loop's cycles and it keeps it at 1.00.
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"
#include "runtime/ee_scheduler.h"
#include "runtime/ps2_perf.h" // GOW-Port: hitch report buckets
#include "raylib.h"

#include <algorithm>
#include <array>
#include <functional>
#include <atomic>
#include <chrono>
#include <clocale>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <thread>
#include <cmath>
#include <cwchar>
#include <mutex>
#include <vector>
#include "gow_aspect.h" // GOW-Port: PC_ASPECT / PC_WIDESCREEN parsing (testable without the game)
#include "gow_osd.inl" // GOW-Port: MSI Afterburner-style performance OSD (GS FPS, GPU, CPU, RAM)
#include "gow_notice.h"  // GOW-Port: short host notices over the game (save backups)
#include "gow_overlay.h" // GOW-Port: live switches of that OSD for the pause menu
#include "gow_persist.h" // GOW-Port: what the player changes in game goes back to the launcher
#include "gow_cheats.h"
#include "gow_image.h"
#include "gow_frame.h"
#include "gow_quality.h"
// GOW-Port: hitch report counters (gs_hw_backend.cpp)
extern std::atomic<uint64_t> g_gowHitchTexDecodes, g_gowHitchTexDecodeNs, g_gowHitchTexTexels, g_gowHitchTargets;
#if defined(_WIN32)
#include <fcntl.h>
#include <io.h>
#endif

namespace
{
    // GOW-Port: the log through a pipe and a writer thread. Writing a line straight to the log file cost the game 50-100 ms
    // in the clang build (the [gow-perf] line every 5 s was a hitch every 5 s, longer as the file grew, likely the
    // antivirus examining the file on each write; the MSVC build did not show it). The game threads now write into a 1 MB
    // pipe and only this thread waits for the file. At exit it drains the pipe, so the last lines are kept.
    // GOW_LOG_DIRECT=1 writes straight to the file as before.
    struct LogWriter
    {
        int original[2] = {-1, -1}; // stdout, stderr as they were (the files the launcher opened)
        std::thread threads[2];
        std::atomic<bool> stopped{false};
    };
    LogWriter g_logWriter;

    // GOW-Port: drains the pipe into the file and puts the original streams back. It runs once: at exit through atexit,
    // and from gowFlushLog() wherever the process ends without atexit (main's std::_Exit, std::terminate). Before it ran
    // only through atexit, which std::_Exit skips, so the last lines of a session could stay in the pipe.
    void stopLogWriter()
    {
#if defined(_WIN32)
        if (g_logWriter.stopped.exchange(true))
            return;
        for (int i = 0; i < 2; ++i)
        {
            if (g_logWriter.original[i] < 0)
                continue;
            std::fflush(i == 0 ? stdout : stderr);
            _dup2(g_logWriter.original[i], i + 1); // closes the pipe's write end: the writer reads to the end and stops
            if (g_logWriter.threads[i].joinable())
                g_logWriter.threads[i].join();
        }
#endif
    }

    void startLogWriter()
    {
#if defined(_WIN32)
        const char *direct = std::getenv("GOW_LOG_DIRECT");
        if (direct && direct[0] == '1')
            return;
        for (int i = 0; i < 2; ++i)
        {
            std::FILE *stream = i == 0 ? stdout : stderr;
            const int fd = _fileno(stream);
            if (fd < 0)
                continue; // no stream (a GUI start without redirection)
            std::fflush(stream);
            const int original = _dup(fd);
            int ends[2];
            if (original < 0 || _pipe(ends, 1u << 20, _O_BINARY) != 0)
            {
                if (original >= 0)
                    _close(original);
                continue;
            }
            if (_dup2(ends[1], fd) != 0)
            {
                _close(ends[0]);
                _close(ends[1]);
                _close(original);
                continue;
            }
            _close(ends[1]);
            g_logWriter.original[i] = original;
            g_logWriter.threads[i] = std::thread([reader = ends[0], original] {
                char buffer[16384];
                for (;;)
                {
                    const int read = _read(reader, buffer, sizeof(buffer));
                    if (read <= 0)
                        break;
                    for (int done = 0; done < read;)
                    {
                        const int wrote = _write(original, buffer + done, static_cast<unsigned>(read - done));
                        if (wrote <= 0)
                            break;
                        done += wrote;
                    }
                }
                _close(reader);
            });
        }
        std::atexit(stopLogWriter);
#endif
    }
}

// GOW-Port: called by main before std::_Exit and by the std::terminate logger before std::abort (src/main.cpp).
void gowFlushLog()
{
    std::fflush(stdout);
    std::fflush(stderr);
    stopLogWriter();
}

namespace ps2_stubs
{
    // GOW-Port: kept out of MemoryCard.h so generated game units do not need rebuilding for a diagnostic hook.
    void logMemoryCardDebugSnapshot();
}

// NOTE: PCKeyboardSticks (ps2_pad.cpp) and the gowIntegration*/gowFrame*/gowWindow*/gowPresentationAspect
// names below are also called from ps2_pad.cpp, ps2_runtime.cpp, Kernel/EeScheduler.cpp and gs/gs_host_present.cpp,
// which are outside this clean-up pass's scope, so they keep their current names for now.
bool PCKeyboardSticks(uint8_t (&sticks)[4]); // ps2_pad.cpp (configurable keyboard)
void gowPadDiagFrame();                       // ps2_pad.cpp (GOW_PAD_DIAG)
void gowCheatsInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowVideoInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowTargetingInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowAnimationCancelInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowPollingInstall(PS2Runtime &runtime);
void gowWaitInstall(PS2Runtime &runtime, const std::string &elfPath);
void gowTargetingInput();
void gowTargetingDraw(float x, float y, float w, float h);
bool gowCheatsInputBlocked();
void gowCheatsMenuInput(bool escClosesGame);
void gowCheatsMenuDraw();
void gowCheatsVBlank(uint8_t *rdram);
void gowRetroAchievementsInstall(const std::string &elfPath);
void gowRetroAchievementsVBlank(uint8_t *rdram);
void gowRetroAchievementsDraw();
void gowSamplingStart(); // gow_sampling.cpp (GOW_SAMPLING)
void gowSamplingBurst(const char *path, double seconds, int intervalUs); // gow_sampling.cpp: synchronous sampling for a few seconds
void gowCrashHandlerInstall(int logFd); // gow_sampling.cpp: [gow-crash] line when the process dies of a fault or abort
int gowLogicFps(); // GOW-Port: below (PC_LOGIC_FPS)
bool gowEarlyFinishAvailable(); // GOW-Port: lib/ps2_memory.cpp (the Frame Rate page of the menus)
bool gowEarlyFinishGet();
void gowEarlyFinishSet(bool enabled);
bool gowVsyncEnabled();
int gowFilterGet(); // GOW-Port: lib/gs/gs_host_present.cpp (the upscaling settings of the Image Quality page)
void gowFilterSet(int mode);
int gowSharpnessGet();
void gowSharpnessSet(int percent);
int gowFxaaGet();
void gowFxaaSet(int on);
float gowGsScaleGet(); // lib/gs/gpu/gs_hw_backend.cpp (internal render scale, applied by the GS at its next draw)
void gowGsScaleSet(float scale);

namespace
{
    bool envFlag(const char *name, bool fallback)
    {
        const char *value = std::getenv(name);
        if (!value || !*value)
            return fallback;
        return value[0] == '1';
    }

    long envLong(const char *name, long fallback, long low, long high)
    {
        const char *value = std::getenv(name);
        if (!value || !*value)
            return fallback;
        return std::clamp(std::strtol(value, nullptr, 10), low, high);
    }

    // GOW-Port: the parsing and the 4:3..4.0 range (32:9 included) are in gow_aspect.h, tested without the game.
    float parseAspect(const char *value) { return gowaspect::parseAspect(value); }

    bool escClosesGame()
    {
        static const bool value = envFlag("PC_ESC_QUITS", true);
        return value;
    }

    void applyCursor()
    {
        if (IsWindowFullscreen() || IsWindowState(FLAG_BORDERLESS_WINDOWED_MODE))
            HideCursor();
        else
            ShowCursor();
    }

    // GOW-Port: GetWindowHandle() returns an HWND on Windows, not a GLFWwindow*. Passing it to glfwSetWindowAttrib caused
    // an access violation when starting from the launcher in fullscreen. raylib's borderless mode
    // covers the monitor without changing its resolution and keeps the previous size for F11.
    void toggleFullscreen()
    {
        ToggleBorderlessWindowed();
        applyCursor();
    }

    // GOW-Port: God of War's pad read (scePad2Read, 0x27B9F0; gowPad2Read in the Waffle overrides). With
    // PC_KEYS the keyboard sticks come from the configurable map; with the cheat menu open the packet
    // stays idle (buttons released, sticks centered, no pressure).
    void reapplyWidescreenAspect(uint8_t *rdram); // further below, next to the widescreen code

    // GOW-Port: which parts of the OSD are shown. The launcher sets the start state (PC_SHOW_FPS, PC_SHOW_GPU, PC_SHOW_CPU,
    // PC_SHOW_RAM); the "In Game Overlay" page of the pause menu changes it live (gowOverlaySet, EE thread) while the render
    // thread reads it. Bits: 1 FPS, 2 GPU, 4 CPU, 8 RAM.
    std::atomic<unsigned> g_osdMask{0};
    gow_osd::Show osdShow()
    {
        const unsigned m = g_osdMask.load(std::memory_order_relaxed);
        return gow_osd::Show{(m & 1u) != 0u, (m & 2u) != 0u, (m & 4u) != 0u, (m & 8u) != 0u};
    }

    // GOW-Port: game frames per second (PC_SHOW_FPS=1): entries to the frame function, 0x1837B8 (the one from
    // GOW_PERF_FRAME_PC). Resumes after a checkpoint (ctx->pc different from the entry) don't count.
    constexpr uint32_t kGameFrameFunction = 0x001837B8u;
    PS2Runtime::RecompiledFunction g_gameFrameFunction = nullptr;
    std::atomic<uint64_t> g_gameFrames{0};
    int g_gameFrameLimit = 60;

    std::atomic<R5900Context *> g_gameFrameContext{nullptr}; // GOW_STALL_DIAG: the main thread's context, read live

    // GOW-Port: what a slow game frame spent its time on, appended to its "stutter of" line: the [gow-perf] buckets of
    // every thread (EE, VU, GS, IOP, waits, upload; with GOW_PERF_DIAG=1, set by the launcher) and the GS texture decodes
    // and new render targets (gs_hw_backend.cpp) between the frame's start and its end. A bucket is charged when its
    // thread leaves it, so a long stretch can land in the next frame.
    struct GowHitchSnapshot
    {
        std::array<uint64_t, ps2_perf::bucketCount> nanos{};
        uint64_t texDecodes = 0, texDecodeNs = 0, texTexels = 0, targets = 0;

        static GowHitchSnapshot take()
        {
            GowHitchSnapshot s;
            for (size_t i = 0; i < ps2_perf::bucketCount; ++i)
                s.nanos[i] = ps2_perf::nanos[i].load(std::memory_order_relaxed);
            s.texDecodes = g_gowHitchTexDecodes.load(std::memory_order_relaxed);
            s.texDecodeNs = g_gowHitchTexDecodeNs.load(std::memory_order_relaxed);
            s.texTexels = g_gowHitchTexTexels.load(std::memory_order_relaxed);
            s.targets = g_gowHitchTargets.load(std::memory_order_relaxed);
            return s;
        }
    };
    GowHitchSnapshot g_gowHitchLast;

    void gowReportHitch(double ms, double at, const GowHitchSnapshot &now, const GowHitchSnapshot &before)
    {
        const auto bucketMs = [&](ps2_perf::Bucket b)
        {
            const size_t i = static_cast<size_t>(b);
            return static_cast<double>(now.nanos[i] - before.nanos[i]) * 1e-6;
        };
        std::fprintf(stderr,
                     "[gow-frames] stutter of %.0f ms at t=%.3f ee=%.1f vu=%.1f gs=%.1f iop=%.1f guest_wait=%.1f upload=%.1f "
                     "host_wait=%.1f tex_decodes=%llu tex_ms=%.1f tex_ktexels=%.0f new_targets=%llu\n",
                     ms, at, bucketMs(ps2_perf::Bucket::Ee), bucketMs(ps2_perf::Bucket::Vu), bucketMs(ps2_perf::Bucket::Gs),
                     bucketMs(ps2_perf::Bucket::Iop), bucketMs(ps2_perf::Bucket::GuestWait), bucketMs(ps2_perf::Bucket::Upload),
                     bucketMs(ps2_perf::Bucket::HostWait), static_cast<unsigned long long>(now.texDecodes - before.texDecodes),
                     static_cast<double>(now.texDecodeNs - before.texDecodeNs) * 1e-6,
                     static_cast<double>(now.texTexels - before.texTexels) / 1000.0,
                     static_cast<unsigned long long>(now.targets - before.targets));
    }

    void gameFrameHook(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        g_gameFrameContext.store(ctx, std::memory_order_relaxed);
        if (ctx->pc == kGameFrameFunction)
        {
            // GOW-Port: limit the game's output, not the window's repeats.
            // VBlank already imposes ~60 Hz. At 30, waiting before entering vid::Flip lets
            // the guest clock advance and the next delta represents two fields.
            if (g_gameFrameLimit == 30)
            {
                using Clock = std::chrono::steady_clock;
                constexpr auto period = std::chrono::nanoseconds(1'000'000'000 / 30);
                static Clock::time_point next{};
                auto now = Clock::now();
                if (next != Clock::time_point{} && now < next)
                {
                    std::this_thread::sleep_until(next);
                    now = Clock::now();
                }
                if (next == Clock::time_point{} || now - next > period)
                    next = now + period;
                else
                    next += period;
            }
            g_gameFrames.fetch_add(1, std::memory_order_relaxed);
            // GOW-Port: [gow-frames] (GOW_FRAMES_DIAG=1): distribution of the period between game frames, per 5 s window,
            // and every long hitch (> 100 ms) as it happens. The [gow-perf] windows hide one-second hitches.
            static const bool framesDiag = envFlag("GOW_FRAMES_DIAG", envFlag("GOW_PERF_DIAG", false)); // el lanzador define GOW_PERF_DIAG=1
            // GOW-Port: GOW_FRAMES_HITCH_LOG=1 alone logs the hitches without the 5 s report (to rule the report out).
            static const bool hitchOnly = !framesDiag && envFlag("GOW_FRAMES_HITCH_LOG", false);
            if (framesDiag || hitchOnly)
            {
                using Clock = std::chrono::steady_clock;
                static const Clock::time_point start = Clock::now();
                static Clock::time_point lastFrame{}, lastReport = start;
                static double maxMs = 0.0;
                static unsigned frames = 0, over25 = 0, over33 = 0, over50 = 0, over100 = 0;
                const auto now = Clock::now();
                if (lastFrame != Clock::time_point{})
                {
                    const double ms = std::chrono::duration<double, std::milli>(now - lastFrame).count();
                    ++frames;
                    maxMs = std::max(maxMs, ms);
                    over25 += ms > 25.0;
                    over33 += ms > 33.4;
                    over50 += ms > 50.0;
                    over100 += ms > 100.0;
                    // GOW-Port: GOW_FRAMES_HITCH_LOG=1 also logs every frame over 50 ms, with the time, to find what they line up with.
                    static const bool hitchLog = envFlag("GOW_FRAMES_HITCH_LOG", false);
                    const GowHitchSnapshot snapshot = GowHitchSnapshot::take();
                    if (ms > 100.0 || (hitchLog && ms > 50.0))
                        gowReportHitch(ms, std::chrono::duration<double>(now - start).count(), snapshot, g_gowHitchLast);
                    g_gowHitchLast = snapshot;
                    if (ms > 1000.0)
                        ps2_stubs::logMemoryCardDebugSnapshot();
                }
                lastFrame = now;
                if (now - lastReport >= std::chrono::seconds(5))
                {
                    if (framesDiag)
                        std::fprintf(stderr, "[gow-frames] t=%.1f frames=%u max_ms=%.1f >25ms=%u >33ms=%u >50ms=%u >100ms=%u\n",
                                 std::chrono::duration<double>(now - start).count(), frames, maxMs, over25, over33, over50, over100);
                    lastReport = now;
                    maxMs = 0.0;
                    frames = over25 = over33 = over50 = over100 = 0;
                }
            }
        }
        g_gameFrameFunction(rdram, ctx, runtime);
    }

    // `restart`: the OSD was hidden until this frame (the pause menu can switch it on mid-game), so the measuring window starts
    // again instead of averaging over the whole time it was off; the FPS line waits for its first full second.
    void drawFps(float x, float y, float h, uint64_t presentedSequence, bool hasSharedFrame, gow_osd::Show show, bool restart)
    {
        static double last = 0.0;
        static uint64_t framesBefore = 0;
        static uint64_t lastPresentedSequence = 0;
        static int gameFps = 0, newFps = 0, windowFps = 0;
        static int newCount = 0, windowCount = 0;
        static bool measured = false;
        const double now = GetTime();
        if (restart)
        {
            last = 0.0;
            gameFps = newFps = windowFps = 0;
            newCount = windowCount = 0;
            measured = false;
        }
        const uint64_t frames = g_gameFrames.load(std::memory_order_relaxed);
        ++windowCount;
        // GOW-Port: 1 % low: intervals between new GS frames (the last 600, ≈ 10 s); computed every second as the FPS that
        // come from the average of the longest 1 % of intervals (like MSI Afterburner / CapFrameX). A repeated frame lengthens the interval.
        static float intervalsMs[600];
        static int intervalCount = 0, intervalHead = 0, lowFps = 0;
        static double lastNewTime = 0.0;
        if (hasSharedFrame && presentedSequence != 0 && presentedSequence != lastPresentedSequence)
        {
            ++newCount;
            lastPresentedSequence = presentedSequence;
            if (lastNewTime > 0.0)
            {
                intervalsMs[intervalHead] = static_cast<float>((now - lastNewTime) * 1000.0);
                intervalHead = (intervalHead + 1) % 600;
                intervalCount = std::min(intervalCount + 1, 600);
            }
            lastNewTime = now;
        }
        if (last == 0.0)
        {
            last = now;
            framesBefore = frames;
        }
        else if (now - last >= 1.0)
        {
            const double interval = now - last;
            // GOW-Port: never above the chosen limit (PC_GAME_FPS=30, 60, or PC_LOGIC_FPS). After a hitch the guest clock
            // catches up the lost time (late VBlanks fire back to back) and a second can count a few extra frames; that is
            // time recovery, not the game running past the limit, so the counter shows the limit.
            const int limit = g_gameFrameLimit == 30 ? 30 : gowLogicFps();
            gameFps = std::min(limit, static_cast<int>(static_cast<double>(frames - framesBefore) / interval + 0.5));
            newFps = static_cast<int>(newCount / interval + 0.5);
            windowFps = static_cast<int>(windowCount / interval + 0.5);
            if (intervalCount >= 30)
            {
                std::vector<float> sorted(intervalsMs, intervalsMs + intervalCount);
                const size_t worst = std::max<size_t>(1u, sorted.size() / 100u);
                std::nth_element(sorted.begin(), sorted.begin() + static_cast<std::ptrdiff_t>(worst - 1u), sorted.end(), std::greater<float>());
                double sum = 0.0;
                for (size_t i = 0; i < worst; ++i)
                    sum += sorted[i];
                const double average = sum / static_cast<double>(worst);
                lowFps = average > 0.0 ? static_cast<int>(1000.0 / average + 0.5) : 0;
            }
            // GOW-Port: [gow-osd] (GOW_PERF_DIAG=1 or GOW_FRAMES_DIAG=1): every second in which what the OSD shows drops below 55, what the
            // user sees (gs = new GS frames, game = entries to the frame function, window = refreshes) is left in the log.
            static const bool osdDiag = envFlag("GOW_FRAMES_DIAG", envFlag("GOW_PERF_DIAG", false));
            if (osdDiag && (newFps < 55 || gameFps < 55))
                std::fprintf(stderr, "[gow-osd] t=%.1f gs=%d game=%d window=%d\n", now, newFps, gameFps, windowFps);
            last = now;
            framesBefore = frames;
            newCount = windowCount = 0;
            measured = true;
        }
        // GOW-Port: the OSD shows one frame rate, the real one: the game's own frames per second (entries to its frame
        // function, which follow the launcher's FPS limit, PC_LOGIC_FPS), plus GPU, CPU and RAM when the system gives them.
        // The new pictures on screen (newFps), the window refresh (windowFps, the monitor's rate) and the 1 % low stay in
        // the [gow-osd] log line only.
        show.fps = show.fps && measured; // never a made-up 0 while the first second is still being counted
        gow_osd::draw(x, y, h, gameFps, -1, -1, -1, show);
    }
    constexpr uint32_t kGowPad2Read = 0x0027B9F0u;
    PS2Runtime::RecompiledFunction g_gowPad2Read = nullptr;

    void PCPad2Read(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t buffer = GPR_U32(ctx, 5) & 0x01FFFFFFu;
        g_gowPad2Read(rdram, ctx, runtime);
        // The Waffle overrides write the 16/9 aspect on every pad read; with PC_WIDESCREEN the chosen
        // one is written again before the game uses it (otherwise it alternates between the two every frame).
        reapplyWidescreenAspect(rdram);
        if (GPR_U32(ctx, 2) != 18u || buffer + 18u > PS2_RAM_SIZE)
            return;
        if (gowCheatsInputBlocked())
        {
            uint8_t *packet = rdram + buffer;
            packet[0] = packet[1] = 0xFFu;
            std::memset(packet + 2u, 0x80, 4u);
            std::memset(packet + 6u, 0, 12u);
            return;
        }
        // GOW-Port: the packet's sticks go in, so that with a gamepad listed only the axes the keyboard moves change.
        uint8_t sticks[4];
        std::memcpy(sticks, rdram + buffer + 2u, sizeof(sticks));
        if (PCKeyboardSticks(sticks))
            std::memcpy(rdram + buffer + 2u, sticks, sizeof(sticks));
    }

    // Widescreen Hor+ (see docs/LAUNCHER.md, "Widescreen (Hor+)"): F at 0x7F00 and the video flag at 0x7F04, every
    // VBlank and like the PCSX2 patch: aspect A at 0x32F1B0 and in both display modes (0x2FD930/0x2FD968),
    // the game's own flag 0x29CC90 = 1, 0x304D94 = 2 and videos without widescreen mode; F = 0.75 (more is seen on the
    // sides) except with the menu background (0x29CC50 = 1), where F = 1/A (vertical crop).
    void gowWidescreenVBlank(uint8_t *rdram);
}

// GOW-Port: the launcher's widescreen switch: PC_WIDESCREEN=A turns on PCSX2's "Widescreen 16:9" patch (Hor+, the
// camera is widened to A). 0 = switch off: the game keeps its original 4:3 picture (gow_pad2.cpp does not force its
// own widescreen mode). A 4:3 or narrower aspect is off as well: there is nothing to widen and, without the switch, the
// game must not be put in the widescreen mode it uses with vertical cropping.
float gowWidescreenAspect()
{
    static const float aspect = [] { return gowaspect::widescreenAspect(std::getenv("PC_WIDESCREEN")); }();
    return aspect;
}

namespace
{
    void gowWidescreenVBlank(uint8_t *rdram)
    {
        if (!rdram)
            return;
        const auto read32 = [rdram](uint32_t address)
        {
            uint32_t value = 0u;
            std::memcpy(&value, rdram + address, sizeof(value));
            return value;
        };
        const auto write32 = [rdram](uint32_t address, uint32_t value) { std::memcpy(rdram + address, &value, sizeof(value)); };
        const auto floatBits = [](float value)
        {
            uint32_t bits = 0u;
            std::memcpy(&bits, &value, sizeof(bits));
            return bits;
        };
        const float target = gowWidescreenAspect();
        float factor = 0.75f;
        // With a video playing (state 11 of the video thread, 0x29C838; 10 is a preloaded one waiting while
        // playing) Waffle's 16:9 mode is kept: with
        // 0x304D94 = 2 the game draws the widescreen video in a 4:3 box (narrow and letterboxed); this fills the image.
        static uint32_t original304D94 = 0xFFFFFFFFu;
        const uint32_t videoState = read32(0x29C838u);
        const bool video = (videoState >= 3u && videoState <= 9u) || videoState == 11u; // loading or playing
        if (target > 0.0f && original304D94 == 0xFFFFFFFFu)
            original304D94 = read32(0x304D94u);
        if (target > 0.0f && video)
        {
            if (original304D94 != 0xFFFFFFFFu)
                write32(0x304D94u, original304D94);
            write32(0x7F04u, 1u);
        }
        else if (target > 0.0f)
        {
            const uint32_t bits = floatBits(target);
            write32(0x32F1B0u, bits);
            write32(0x2FD930u, bits);
            write32(0x2FD968u, bits);
            rdram[0x29CC90u] = 1u;
            // 0x304D94 = 2 (PCSX2 patch) makes the game put videos in a 4:3 box; with Waffle's native 16:9 it isn't
            // needed. PC_WIDESCREEN_304D94=1 writes it again (for comparisons).
            static const bool write304D94 = [] { const char *v = std::getenv("PC_WIDESCREEN_304D94"); return v && v[0] == '1'; }();
            if (write304D94)
                write32(0x304D94u, 2u);
            // Widescreen videos (.PSW): the Waffle runtime plays them natively and they fill the image;
            // the previous implementation used 4:3 because FFmpeg drew the video on top at its own aspect ratio.
            write32(0x7F04u, 1u);
            uint16_t menuBackground = 0u;
            std::memcpy(&menuBackground, rdram + 0x29CC50u, sizeof(menuBackground));
            if (menuBackground == 1u)
                factor = 1.0f / target;
        }
        else
        {
            const uint32_t bits = read32(0x32F1B0u);
            float aspect = 0.0f;
            std::memcpy(&aspect, &bits, sizeof(aspect));
            if (bits == 0x3FE38E39u) // 16/9: the game's own widescreen mode
                factor = 0.5625f;
            else if (bits != 0x3FAAAAABu && aspect > 0.5f && aspect < 4.0f) // 4/3 or still unwritten: 0.75
                factor = 1.0f / aspect;
            write32(0x7F04u, read32(0x29CC90u));
        }
        write32(0x7F00u, floatBits(factor));
    }
}

namespace
{
    void reapplyWidescreenAspect(uint8_t *rdram)
    {
        const float target = gowWidescreenAspect();
        if (target <= 0.0f || !rdram)
            return;
        uint32_t video = 0u;
        std::memcpy(&video, rdram + 0x29C838u, sizeof(video));
        if ((video >= 3u && video <= 9u) || video == 11u) // video loading or playing: Waffle's 16/9
            return;
        uint32_t bits = 0u;
        std::memcpy(&bits, &target, sizeof(bits));
        for (const uint32_t address : {0x32F1B0u, 0x2FD930u, 0x2FD968u})
            std::memcpy(rdram + address, &bits, sizeof(bits));
    }
}

// GOW-Port: one extra EE-thread callback per VBlank for the game overrides (src/game), registered at startup.
static void (*g_vblankExtra)(uint8_t *) = nullptr;
void gowSetVBlankExtra(void (*callback)(uint8_t *rdram))
{
    g_vblankExtra = callback;
}

// GOW-Port: Kernel/EeScheduler.cpp, at the start of every VBlank and when the scheduler restarts.
void gowIntegrationVBlank(uint8_t *rdram)
{
    gowWidescreenVBlank(rdram);
    gowCheatsVBlank(rdram);
    if (g_vblankExtra)
        g_vblankExtra(rdram);
    gowRetroAchievementsVBlank(rdram);
}

// GOW-Port: window image aspect ratio (gs_host_present.cpp). 0 = stretch to fill the window.
float gowPresentationAspect()
{
    static const float aspect = []
    {
        if (const char *value = std::getenv("PC_ASPECT"); value && *value && std::strcmp(value, "auto") != 0)
        {
            if (std::strcmp(value, "stretch") == 0)
                return 0.0f;
            if (const float fixed = parseAspect(value); fixed > 0.0f)
                return fixed;
        }
        if (const float wide = gowWidescreenAspect(); wide > 0.0f)
            return wide;
        // GOW-Port: with the widescreen switch off the game draws its original 4:3 picture (gow_pad2.cpp): presenting it
        // as 16:9 would stretch it, so the picture keeps its own geometry and the window shows side bars.
        return 4.0f / 3.0f;
    }();
    return aspect;
}

// GOW-Port: ps2_runtime.cpp (initialize), in place of Waffle's InitWindow.
void gowWindowOpen(const char *title)
{
    int width = static_cast<int>(envLong("PC_WIDTH", 1920, 320, 7680));
    int height = static_cast<int>(envLong("PC_HEIGHT", 1080, 224, 4320));
    if (!std::getenv("PC_WIDTH") && !std::getenv("PC_HEIGHT"))
        if (const char *size = std::getenv("GOW_WINDOW"))
        {
            int w = 0, h = 0;
            if (std::sscanf(size, "%dx%d", &w, &h) == 2 && w >= 320 && h >= 240)
                width = w, height = h;
        }
    const char *mode = std::getenv("PC_SCREEN");
    const bool fullscreen = mode && std::strcmp(mode, "fullscreen") == 0;
    SetConfigFlags(FLAG_WINDOW_RESIZABLE | (envFlag("PC_VSYNC", false) ? FLAG_VSYNC_HINT : 0u));
    const char *customTitle = std::getenv("PC_TITLE");
    InitWindow(width, height, customTitle && *customTitle ? customTitle : title);
    // GOW-Port: GLFW sets the system's LC_CTYPE. With tr_TR, tolower('I') is 'ı' and toupper('i') is 'İ': ASCII
    // name comparisons (IRX modules, disc files) were failing. The game doesn't use text input.
    std::setlocale(LC_CTYPE, "C");
    if (!IsWindowReady())
        return;
    const int monitor = GetCurrentMonitor();
    const int maxWidth = GetMonitorWidth(monitor), maxHeight = GetMonitorHeight(monitor);
    if (!fullscreen && maxWidth > 0 && maxHeight > 0 && (width > maxWidth || height > maxHeight))
    {
        const float scale = std::min(static_cast<float>(maxWidth) / width, static_cast<float>(maxHeight) / height);
        SetWindowSize(static_cast<int>(width * scale), static_cast<int>(height * scale));
        SetWindowPosition(0, 0);
    }
    SetWindowMinSize(320, 224);
    if (const char *icon = std::getenv("PC_ICON"); icon && *icon)
    {
        Image image = LoadImage(icon);
        if (image.data)
        {
            SetWindowIcon(image);
            UnloadImage(image);
        }
    }
    if (fullscreen)
        toggleFullscreen();
    if (!escClosesGame())
        SetExitKey(KEY_NULL);
}

// GOW-Port: window frames per second (Waffle: 60), at most kMaxFps: the project never runs above 120 (AGENTS.md). Unlimited (0)
// and faster displays get 120: past the game's own rate the window only repeats pictures, and an uncapped window kept the GPU
// at 100 % until the AMD OpenGL driver crashed (atio6axx.dll, every run of a 240 FPS A/B on 4 cores).
int gowWindowFps()
{
    const char *value = std::getenv("PC_WINDOW_FPS");
    if (!value || !*value)
        return 60;
    const long fps = std::strtol(value, nullptr, 10);
    return fps <= 0 ? kMaxFps : static_cast<int>(std::clamp<long>(fps, 30, kMaxFps));
}

// GOW-Port: game logic rate (PC_LOGIC_FPS, experimental and off by default; src/game/gow_logic_rate.cpp). 60 unless
// the value is one of the supported rates and the 30 FPS cap (PC_GAME_FPS=30) is off. Read by the overrides and by the
// scheduler's pacing (Kernel/EeScheduler.cpp).
namespace
{
    // The logic rate as the launcher started it (PC_LOGIC_FPS) and as the Frame Rate page of the menus left it.
    int startingLogicFps()
    {
        static const int fps = [] {
            const long requested = envLong("PC_LOGIC_FPS", 60, 0, 1000);
            if (requested == 60)
                return 60;
            // GOW-Port: 120 is the only rate above 60; a higher request (144, 165 and 240 used to exist) gets 120, the cap.
            if (requested > kMaxFps)
            {
                std::fprintf(stderr, "[gow-hfps] PC_LOGIC_FPS=%ld is above the %d FPS cap: %d\n", requested, kMaxFps, kMaxFps);
                return kMaxFps;
            }
            if (requested != kMaxFps)
            {
                std::fprintf(stderr, "[gow-hfps] PC_LOGIC_FPS=%ld is not supported (60 or %d): 60\n", requested, kMaxFps);
                return 60;
            }
            if (envLong("PC_GAME_FPS", 60, 30, 60) <= 30)
            {
                std::fprintf(stderr, "[gow-hfps] PC_LOGIC_FPS=%ld ignored with PC_GAME_FPS=30: 60\n", requested);
                return 60;
            }
            return static_cast<int>(requested);
        }();
        return fps;
    }
    std::atomic<int> g_logicFps{0}; // 0 = not read yet
}

// GOW-Port: game logic rate (PC_LOGIC_FPS, experimental; src/game/gow_logic_rate.cpp). 60 unless the value is one of the supported
// rates and the 30 FPS cap (PC_GAME_FPS=30) is off. Read by the overrides, the scheduler's pacing (Kernel/EeScheduler.cpp) and the
// presentation; the Frame Rate page of the menus changes it while the game runs (gowFrameSet), so none of them caches it.
int gowLogicFps()
{
    int fps = g_logicFps.load(std::memory_order_relaxed);
    if (fps == 0)
    {
        fps = startingLogicFps();
        int expected = 0;
        g_logicFps.compare_exchange_strong(expected, fps);
        fps = g_logicFps.load(std::memory_order_relaxed);
    }
    return fps;
}

// GOW-Port: the minimum time between two new frames in the window (gs_gl.cpp). The launcher sets GOW_PRESENT_MIN_INTERVAL_US to 80 %
// of a frame at its FPS limit (800000 / fps); when that is the value in the environment, the interval follows the limit the menu
// chooses (gowPresentMinIntervalSet), a value anyone else set (diagnostics) is kept as it is.
void gowPresentMinIntervalSet(long microseconds); // lib/gs/gpu/gs_gl.cpp
static void followPresentInterval(int fps)
{
    static const char *const text = std::getenv("GOW_PRESENT_MIN_INTERVAL_US");
    static const bool followsTheLimit = text && std::strtol(text, nullptr, 10) == 800000L / startingLogicFps();
    if (followsTheLimit)
        gowPresentMinIntervalSet(800000L / fps);
}

// GOW-Port: window VSync, switched by the menus (ps2_runtime.cpp asks it for the pacing mode, gowFrameInput applies it to the window).
namespace
{
    std::atomic<bool> g_vsyncWanted{false};
    std::atomic<bool> g_vsyncApplied{false};
    std::atomic<bool> g_vsyncStarted{false};
}

bool gowVsyncEnabled()
{
    if (!g_vsyncStarted.load(std::memory_order_acquire))
    {
        g_vsyncWanted.store(envFlag("PC_VSYNC", false));
        g_vsyncApplied.store(g_vsyncWanted.load());
        g_vsyncStarted.store(true, std::memory_order_release);
    }
    return g_vsyncWanted.load(std::memory_order_relaxed);
}

int gowFrameCount()
{
    return 3;
}

bool gowFrameAvailable(int index)
{
    if (index == 0)
        return envLong("PC_GAME_FPS", 60, 30, 60) > 30;
    if (index == 2)
        return gowEarlyFinishAvailable();
    return index == 1;
}

int gowFrameGet(int index)
{
    if (index == 0)
        return gowLogicFps() <= 90 ? 0 : 1;
    if (index == 1)
        return gowVsyncEnabled() ? 1 : 0;
    if (index == 2)
        return gowEarlyFinishGet() ? 1 : 0;
    return 0;
}

void gowFrameSet(int index, int value)
{
    if (!gowFrameAvailable(index))
        return;
    if (index == 0)
    {
        static const int rates[2] = {60, kMaxFps};
        const int next = rates[std::clamp(value, 0, 1)];
        if (gowLogicFps() != next)
        {
            g_logicFps.store(next, std::memory_order_relaxed);
            followPresentInterval(next);
            std::fprintf(stderr, "[gow-hfps] FPS limit %d (changed in game)\n", next);
            gowPersistTouch();
        }
    }
    else if (index == 1)
    {
        if (gowVsyncEnabled() != (value != 0))
        {
            g_vsyncWanted.store(value != 0, std::memory_order_relaxed); // applied by the window loop (gowFrameInput)
            gowPersistTouch();
        }
    }
    else if (index == 2)
    {
        if (gowEarlyFinishGet() != (value != 0))
        {
            gowEarlyFinishSet(value != 0);
            gowPersistTouch();
        }
    }
}

int gowQualityCount()
{
    return 4;
}

int gowQualityGet(int index)
{
    switch (index)
    {
    case 0: return std::clamp(static_cast<int>(std::lround(gowGsScaleGet())) - 1, 0, 7);
    case 1: return gowFilterGet();
    case 2: return gowSharpnessGet();
    case 3: return gowFxaaGet();
    default: return 0;
    }
}

void gowQualitySet(int index, int value)
{
    const int before = gowQualityGet(index);
    switch (index)
    {
    case 0: gowGsScaleSet(static_cast<float>(std::clamp(value, 0, 7) + 1)); break;
    case 1: gowFilterSet(value); break;
    case 2: gowSharpnessSet(value); break;
    case 3: gowFxaaSet(value); break;
    default: return;
    }
    if (gowQualityGet(index) != before || index == 0)
        gowPersistTouch(); // the launcher keeps it for the next session
}

// GOW-Port: game buffer swaps (the display setup 0x183360 after it handed the new DISPFB to the presentation; called by
// src/game/gow_display.cpp). With PC_LOGIC_FPS > 60 the window latches a new picture on each swap, not only on each
// 60 Hz VBlank (gs_host_present.cpp).
static std::atomic<uint64_t> g_gowGameFlips{0};

void gowNoteGameFlip()
{
    g_gowGameFlips.fetch_add(1, std::memory_order_release);
}

uint64_t gowGameFlipCount()
{
    return g_gowGameFlips.load(std::memory_order_acquire);
}

// GOW-Port: extra [gow-hang] lines from a diagnostic of the game overrides (GOW_LOADER_DIAG: src/game/gow_diag_io.cpp).
static std::atomic<void (*)(PS2Runtime &)> g_gowHangReportExtra{nullptr};

void gowSetHangReportExtra(void (*report)(PS2Runtime &))
{
    g_gowHangReportExtra.store(report, std::memory_order_release);
}

// GOW-Port: the hang after saving (0.3.x, Windows) was sampled with the EE spinning inside the animation controller update
// 0x116EF0 (this = s1: +0x8 time, +0xC rate, +0x10 previous time, +0x38 animation, +0x58 vtable), in the loop at
// 0x117358..0x117384 that walks the time from the previous value to the new one in steps of animation+0x70, calling the
// vtable+0x14 method per step, until |target - x| <= step. It never ends if the step is zero or tiny, or if the times are so
// large that adding the step no longer changes them. This prints, once per hang, what decides it: the live FPU registers of the
// loop (f20 = x, f21 = step, f22 = target, f24 = signed step, f12 = previous), the controller's fields and the method called.
// Read from the watchdog thread while the EE spins in that loop (it touches neither the thread lists nor the scheduler then):
// the values can be a step apart, which does not matter for telling a zero step from an absorbed one.
static void gowHangReportAnimationStep(PS2Runtime &runtime)
{
    const R5900Context *ctx = runtime.eeScheduler().currentContext();
    const uint8_t *rdram = runtime.memory().getRDRAM();
    if (!ctx || !rdram)
        return;
    auto bits = [](float value) { uint32_t b = 0u; std::memcpy(&b, &value, sizeof(b)); return b; };
    auto word = [&](uint32_t address) { uint32_t v = 0u; std::memcpy(&v, rdram + (address & 0x01FFFFFCu), sizeof(v)); return v; };
    auto real = [&](uint32_t address) { float v = 0.0f; std::memcpy(&v, rdram + (address & 0x01FFFFFCu), sizeof(v)); return v; };
    auto half = [&](uint32_t address) { int16_t v = 0; std::memcpy(&v, rdram + (address & 0x01FFFFFEu), sizeof(v)); return v; };
    const uint32_t self = static_cast<uint32_t>(_mm_cvtsi128_si32(ctx->r[17])) & 0x01FFFFFFu;
    std::fprintf(stderr, "[gow-hang] animation step loop: pc=%08x x=%g (%08x) step=%g (%08x) target=%g (%08x) signed_step=%g previous=%g fcr31=%08x\n",
                 ctx->pc, ctx->f[20], bits(ctx->f[20]), ctx->f[21], bits(ctx->f[21]), ctx->f[22], bits(ctx->f[22]), ctx->f[24], ctx->f[12],
                 ctx->fcr31);
    if (self == 0u)
        return;
    const uint32_t animation = word(self + 0x38u) & 0x01FFFFFFu, vtable = word(self + 0x58u) & 0x01FFFFFFu;
    float delta = 0.0f;
    std::memcpy(&delta, rdram + 0x29C64Cu, sizeof(delta));
    std::fprintf(stderr, "[gow-hang] animation controller %08x: time=%g rate=%g previous=%g flags=%08x +0x1c=%g +0x24=%g animation=%08x step=%g length=%g "
                         "method=%08x (this%+d) frame_delta=%g\n",
                 self, real(self + 0x8u), real(self + 0xCu), real(self + 0x10u), word(self + 0x2Cu), real(self + 0x1Cu), real(self + 0x24u), animation,
                 animation ? real(animation + 0x70u) : 0.0f, animation ? real(animation + 0x1Cu) : 0.0f, vtable ? word(vtable + 0x14u) : 0u,
                 vtable ? half(vtable + 0x10u) : 0, delta);
}

// GOW-Port: what the EE is waiting for, printed by the hang watchdog before its sampling burst. Reads: the game's "GS done"
// flag (0x29C7D0: 1 = waiting for the GS FINISH interrupt, 2 = done), the GS status and interrupt mask, the VBlank counter and
// the guest threads of the kernel snapshot. A frame wait with the flag at 1 and CSR.FINISH (bit 1) clear means the FINISH was
// consumed before the game waited for it; with CSR.FINISH set and unmasked in IMR (bit 9 clear) the interrupt was not delivered or
// not acknowledged (the [gow-gs-irq] lines from the scheduler say which). `vsyncStep` is the VBlank ticks seen in the last second:
// 0 means the EE's own clock stopped as well.
static void gowHangReport(PS2Runtime &runtime, uint64_t vsyncStep, uint64_t frames)
{
    PS2Memory &memory = runtime.memory();
    uint32_t flag = 0u, movie = 0u;
    if (const uint8_t *rdram = memory.getRDRAM())
    {
        std::memcpy(&flag, rdram + 0x29C7D0u, sizeof(flag));
        std::memcpy(&movie, rdram + 0x29C838u, sizeof(movie));
    }
    auto &gs = memory.gs();
    std::fprintf(stderr, "[gow-hang] gs_done_flag=%u movie_state=%u csr=%llx imr=%llx vsync_tick=%llu (+%llu in the last second) frames=%llu\n", flag, movie,
                 static_cast<unsigned long long>(gs.csr.load(std::memory_order_acquire)), static_cast<unsigned long long>(gs.imr),
                 static_cast<unsigned long long>(gs.vsyncTick.load(std::memory_order_acquire)), static_cast<unsigned long long>(vsyncStep),
                 static_cast<unsigned long long>(frames));
    const EeKernelSnapshot snapshot = runtime.eeScheduler().snapshot();
    std::fprintf(stderr, "[gow-hang] ee cycle=%llu next_event_cycle=%llu running_thread=%d threads=%zu\n",
                 static_cast<unsigned long long>(snapshot.eeCycle), static_cast<unsigned long long>(snapshot.nextEventCycle),
                 snapshot.runningThreadId, snapshot.threads.size());
    static const char *const kStatus[] = {"running", "ready", "waiting", "waiting+suspended", "suspended", "dormant"};
    static const char *const kWait[] = {"none", "sleep", "semaphore", "eventflag", "vsync", "external", "mpeg"};
    for (const EeThreadSnapshot &thread : snapshot.threads)
    {
        if (thread.status == EeThreadStatus::Dormant)
            continue;
        const size_t status = static_cast<size_t>(thread.status), wait = static_cast<size_t>(thread.waitReason);
        std::fprintf(stderr, "[gow-hang] thread %d %s wait=%s(%d) priority=%d pc=%08x ra=%08x sp=%08x invocations=%u suspend=%d wakeups=%u\n",
                     thread.id, status < 6 ? kStatus[status] : "?", wait < 7 ? kWait[wait] : "?", thread.waitId, thread.currentPriority,
                     thread.pc, thread.ra, thread.sp, thread.invocationDepth, thread.suspendCount, thread.wakeupCount);
    }
    for (const EeSemaphoreSnapshot &semaphore : snapshot.semaphores)
        if (semaphore.waiters != 0u)
            std::fprintf(stderr, "[gow-hang] semaphore %d count=%d/%d waiters=%u\n", semaphore.id, semaphore.count, semaphore.maxCount,
                         semaphore.waiters);
    for (const EeEventFlagSnapshot &eventFlag : snapshot.eventFlags)
        if (eventFlag.waiters != 0u)
            std::fprintf(stderr, "[gow-hang] event flag %d bits=%08x waiters=%u\n", eventFlag.id, eventFlag.bits, eventFlag.waiters);
    for (const EeThreadSnapshot &thread : snapshot.threads)
        if (thread.id == snapshot.runningThreadId && thread.pc >= 0x116EF0u && thread.pc < 0x117438u)
            gowHangReportAnimationStep(runtime);
    if (void (*extra)(PS2Runtime &) = g_gowHangReportExtra.load(std::memory_order_acquire))
        extra(runtime);
}

// GOW-Port: ps2_runtime.cpp (loadELF), after the Waffle overrides (applyMatching): each wrapper calls the
// previous one.
void gowIntegrationInstall(PS2Runtime &runtime, const std::string &elfPath)
{
    startLogWriter(); // GOW-Port: the log through a writer thread (see LogWriter)
    // GOW-Port: a crash used to leave no trace in last.log (issue #35: the log just stops). The handler writes straight
    // into the file the launcher opened, after the writer thread had time to empty the pipe.
    gowCrashHandlerInstall(g_logWriter.original[1]);
    gowSamplingStart(); // optional sampling profiler (GOW_SAMPLING=<file>)
    if (runtime.hasFunction(kGowPad2Read))
    {
        if (PS2Runtime::RecompiledFunction actual = runtime.lookupFunction(kGowPad2Read))
        {
            g_gowPad2Read = actual;
            runtime.replaceFunction(kGowPad2Read, &PCPad2Read);
            if (std::getenv("PC_KEYS"))
                std::cerr << "[teclado] configurable map active (PC_KEYS); keyboard sticks from the map" << std::endl;
        }
    }
    g_gameFrameLimit = envLong("PC_GAME_FPS", 60, 30, 60) <= 30 ? 30 : 60;
    // GOW-Port: a part the launcher does not mention follows PC_SHOW_FPS, which used to show the whole OSD: an older launcher
    // (FPS switch only) keeps GPU, CPU and RAM. The current launcher sends 0 or 1 for each part.
    {
        const bool fps = envFlag("PC_SHOW_FPS", false);
        g_osdMask.store((fps ? 1u : 0u) | (envFlag("PC_SHOW_GPU", fps) ? 2u : 0u) | (envFlag("PC_SHOW_CPU", fps) ? 4u : 0u) |
                        (envFlag("PC_SHOW_RAM", fps) ? 8u : 0u));
    }
    // GOW-Port: with the native pause menu (GOW_NATIVE_CHEATS_MENU=1, which the launcher always sets) the frame counter is
    // always installed: the "In Game Overlay" page can switch the FPS counter on in the middle of a session. The hook costs one
    // relaxed atomic increment per game frame.
    if ((g_gameFrameLimit == 30 || osdShow().any() || envFlag("GOW_NATIVE_CHEATS_MENU", false)) && runtime.hasFunction(kGameFrameFunction))
        if (PS2Runtime::RecompiledFunction actual = runtime.lookupFunction(kGameFrameFunction))
        {
            g_gameFrameFunction = actual;
            runtime.replaceFunction(kGameFrameFunction, &gameFrameHook);
        }
    // GOW-Port: hang watchdog (GOW_PERF_DIAG=1 or GOW_FRAMES_DIAG=1): if the game stops entering the frame function for
    // 5 s, it samples the process's threads for 3 s (gow_hang_<n>.sampling, in the working folder; summarize with
    // ps2-dev sampling-win and gow-2005.map) to see where the EE spins and what the others wait for.
    if (g_gameFrameFunction && (envFlag("GOW_FRAMES_DIAG", false) || envFlag("GOW_PERF_DIAG", false)))
        std::thread([rt = &runtime]
                    {
                        uint64_t last = 0, lastTick = 0, tickStep = 0;
                        int still = 0, dumps = 0;
                        for (;;)
                        {
                            std::this_thread::sleep_for(std::chrono::seconds(1));
                            const uint64_t now = g_gameFrames.load(std::memory_order_relaxed);
                            const uint64_t tick = rt->memory().gs().vsyncTick.load(std::memory_order_relaxed);
                            tickStep = tick - lastTick;
                            lastTick = tick;
                            // GOW-Port: a movie is not a hang. While one plays (movie state 0x29C838: 10 loaded, 11 playing) the game
                            // does not enter its frame function and the movie thread polls its stream service flat out, which looked
                            // exactly like a freeze: the 31 s "hang" after a cutscene started (log of 3 Nov 2026) was the movie itself.
                            uint32_t movieState = 0u;
                            if (const uint8_t *rdram = rt->memory().getRDRAM())
                                std::memcpy(&movieState, rdram + 0x0029C838u, sizeof(movieState));
                            if (now != last || now == 0 || movieState == 10u || movieState == 11u)
                                still = 0;
                            else if (++still == 5 && dumps < 3)
                            {
                                char name[64];
                                std::snprintf(name, sizeof(name), "gow_hang_%d.sampling", ++dumps);
                                std::fprintf(stderr, "[gow-frames] the game has not entered the frame function for 5 s: 3 s sampling -> %s\n", name);
                                gowHangReport(*rt, tickStep, now);
                                gowSamplingBurst(name, 3.0, 1000);
                            }
                            last = now;
                        }
                    })
            .detach();
    // GOW-Port: GOW_STALL_DIAG=1 (diagnostics): while the game has not entered its frame function for more than 40 ms, the
    // running guest thread's pc/ra every 5 ms; when the frame comes, one [gow-stall] line with the gap, the game's display
    // swaps and VBlanks during it and the most frequent places, to find what a short pause waits for.
    if (g_gameFrameFunction && envFlag("GOW_STALL_DIAG", false))
        std::thread([rt = &runtime]
                    {
                        using Clock = std::chrono::steady_clock;
                        uint64_t lastFrames = g_gameFrames.load(std::memory_order_relaxed);
                        Clock::time_point lastChange = Clock::now();
                        uint64_t flipsAtFrame = gowGameFlipCount();
                        uint64_t ticksAtFrame = rt->memory().gs().vsyncTick.load(std::memory_order_relaxed);
                        std::vector<std::pair<uint64_t, unsigned>> places; // (pc << 32 | ra, samples)
                        for (;;)
                        {
                            std::this_thread::sleep_for(std::chrono::milliseconds(5));
                            const uint64_t frames = g_gameFrames.load(std::memory_order_relaxed);
                            const auto now = Clock::now();
                            if (frames != lastFrames)
                            {
                                const double gapMs = std::chrono::duration<double, std::milli>(now - lastChange).count();
                                const uint64_t flips = gowGameFlipCount();
                                const uint64_t ticks = rt->memory().gs().vsyncTick.load(std::memory_order_relaxed);
                                if (!places.empty() && gapMs > 50.0)
                                {
                                    std::sort(places.begin(), places.end(), [](const auto &a, const auto &b) { return a.second > b.second; });
                                    std::string top;
                                    for (size_t i = 0; i < places.size() && i < 6; ++i)
                                    {
                                        char item[48];
                                        std::snprintf(item, sizeof(item), " %08x/%08x x%u", static_cast<uint32_t>(places[i].first >> 32),
                                                      static_cast<uint32_t>(places[i].first), places[i].second);
                                        top += item;
                                    }
                                    std::fprintf(stderr, "[gow-stall] gap=%.0f ms flips=%llu vblanks=%llu pc/ra:%s\n", gapMs,
                                                 static_cast<unsigned long long>(flips - flipsAtFrame),
                                                 static_cast<unsigned long long>(ticks - ticksAtFrame), top.c_str());
                                }
                                places.clear();
                                lastFrames = frames;
                                lastChange = now;
                                flipsAtFrame = flips;
                                ticksAtFrame = ticks;
                                continue;
                            }
                            if (now - lastChange < std::chrono::milliseconds(40))
                                continue;
                            if (places.empty())
                                if (const uint8_t *rdram = rt->memory().getRDRAM())
                                {
                                    // the game's frame pacing words: target rate, VBlanks per frame, VBlank count, flags
                                    uint32_t w[6];
                                    for (int i = 0; i < 6; ++i)
                                        std::memcpy(&w[i], rdram + (0x0029C640u + 4u * i), 4u);
                                    uint32_t count = 0u;
                                    std::memcpy(&count, rdram + 0x0029C7D4u, 4u);
                                    std::fprintf(stderr, "[gow-stall] at %.0f ms: 29C640..654=%u %u %u %u %u %u vblank_count=%u\n",
                                                 std::chrono::duration<double, std::milli>(now - lastChange).count(), w[0], w[1], w[2], w[3],
                                                 w[4], w[5], count);
                                }
                            // the live pc/ra of the main thread's context (racy reads: a diagnostic, not a value to act on)
                            if (const R5900Context *ctx = g_gameFrameContext.load(std::memory_order_relaxed))
                            {
                                const uint64_t key = (uint64_t(*reinterpret_cast<const volatile uint32_t *>(&ctx->pc)) << 32) |
                                                     GPR_U32(ctx, 31);
                                auto it = std::find_if(places.begin(), places.end(), [key](const auto &p) { return p.first == key; });
                                if (it == places.end())
                                    places.emplace_back(key, 1u);
                                else
                                    ++it->second;
                            }
                        }
                    })
            .detach();
    gowCheatsInstall(runtime, elfPath);
    gowRetroAchievementsInstall(elfPath); // login, hash (ISO or ELF) and RDRAM evaluation on VBlank
    gowVideoInstall(runtime, elfPath);    // progressive scan always enabled
    gowTargetingInstall(runtime, elfPath);    // target lock-on (wraps the pad read)
    gowAnimationCancelInstall(runtime, elfPath); // cancel attacks (after fijar: chains its wrapper)
    gowPollingInstall(runtime);            // GOW_POLLING (diagnostics)
    // GOW-Port: the EE's wait for the frame sleeps instead of spinning (gow_wait.cpp), by default. Spinning occupied a whole logical thread
    // and took clock away from the VU1 one; with GOW_EE_WAIT=0 the original loop returns. Combat (26 Oct 2026): 59.1 -> 59.9 fps on average,
    // minimum 50 -> 55.4.
    if (envFlag("GOW_EE_WAIT", true))
        gowWaitInstall(runtime, elfPath);
}

// GOW-Port: ps2_runtime.cpp (window loop), before drawing every frame.
bool gowHostSyncToVBlank(); // GOW-Port: lib/ps2_runtime.cpp

// GOW-Port: window loop (render thread): applies the VSync the menus asked for, and the window pacing that goes with it (with VSync
// the display paces the loop, without it the game's VBlank does: ps2_runtime.cpp).
static void gowApplyFrameSettings()
{
    const bool wanted = gowVsyncEnabled();
    if (wanted == g_vsyncApplied.load(std::memory_order_relaxed))
        return;
    g_vsyncApplied.store(wanted, std::memory_order_relaxed);
    if (wanted)
        SetWindowState(FLAG_VSYNC_HINT);
    else
        ClearWindowState(FLAG_VSYNC_HINT);
    SetTargetFPS(gowHostSyncToVBlank() ? 0 : gowWindowFps());
    std::fprintf(stderr, "[gow-frame] VSync %s (changed in game)\n", wanted ? "on" : "off");
}

void gowFrameInput()
{
    if (IsKeyPressed(KEY_F11))
        toggleFullscreen();
    gowApplyFrameSettings();             // the VSync chosen in the menus
    gowPadDiagFrame();                   // GOW_PAD_DIAG: what GLFW and raylib see of the joysticks
    gowCheatsMenuInput(escClosesGame()); // F2, cheat menu
    gowTargetingInput();                     // Alt locks the target, the mouse switches it
}

// GOW-Port: gs_host_present.cpp (renderFrame), on top of the game image (destination = image rectangle).
void gowFrameDraw(float x, float y, float w, float h, uint64_t presentedSequence, bool hasSharedFrame)
{
    gowTargetingDraw(x, y, w, h);
    gowCheatsMenuDraw();
    gowRetroAchievementsDraw();
    gowNoticeDraw();
    // GOW-Port: a 30 FPS limit (or the native menu) installs the hook without the OSD being on
    static bool osdWasShown = false;
    const gow_osd::Show show = osdShow();
    if (g_gameFrameFunction && show.any())
    {
        drawFps(x, y, h, presentedSequence, hasSharedFrame, show, !osdWasShown);
        osdWasShown = true;
    }
    else
        osdWasShown = false;
}

// GOW-Port: gow_overlay.h, for the "In Game Overlay" page of the pause menu.
int gowOverlayCount()
{
    return 4;
}

bool gowOverlayGet(int index)
{
    return index >= 0 && index < 4 && ((g_osdMask.load(std::memory_order_relaxed) >> index) & 1u) != 0u;
}

void gowOverlaySet(int index, bool shown)
{
    if (index < 0 || index >= 4)
        return;
    const unsigned bit = 1u << index;
    const unsigned before = shown ? g_osdMask.fetch_or(bit, std::memory_order_relaxed) : g_osdMask.fetch_and(~bit, std::memory_order_relaxed);
    if (((before & bit) != 0u) != shown)
        gowPersistTouch();
}

// GOW-Port: what the player changes inside the game (cheats: F2 menu and Port Options pages; overlay lines; image adjustments) is
// written to the file named by GOW_INGAME_STATE (the launcher sets it, and reads the file when the game ends: gow-core/src/ingame.rs).
// A change only marks the state dirty; a thread writes the file once the changes have been quiet for 0.4 s (a slider held down
// changes the value ten times a second), and a last write runs at exit. Without the variable nothing is written.
namespace
{
    std::atomic<bool> g_persistDirty{false};
    std::atomic<long long> g_persistTouchedMs{0};
    std::mutex g_persistWrite;

    long long persistNowMs()
    {
        return std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
    }

    const std::string &persistPath()
    {
        static const std::string path = [] {
            const char *v = std::getenv("GOW_INGAME_STATE");
            return std::string(v ? v : "");
        }();
        return path;
    }

    void persistWriteFile()
    {
        const std::lock_guard<std::mutex> lock(g_persistWrite);
        std::string json = "{\"version\":1";
        if (gowCheatsAvailable()) // under Hardcore the cheat list is not sent at all
        {
            std::string list;
            for (int i = 0; i < gowCheatsCount(); ++i)
                if (gowCheatsGet(i))
                    list += std::string(list.empty() ? "" : ",") + gowCheatsId(i);
            json += ",\"cheats\":\"" + list + "\"";
        }
        static const char *const keys[4] = {"showFps", "showGpu", "showCpu", "showRam"};
        for (int i = 0; i < 4; ++i)
            json += std::string(",\"") + keys[i] + "\":" + (gowOverlayGet(i) ? "true" : "false");
        static const char *const image[4] = {"brightness", "contrast", "gamma", "saturation"};
        for (int i = 0; i < 4; ++i)
            json += std::string(",\"") + image[i] + "\":" + std::to_string(gowImageGet(i));
        static const int limits[2] = {60, kMaxFps};
        json += std::string(",\"fpsLimit\":") + std::to_string(limits[std::clamp(gowFrameGet(0), 0, 1)]);
        json += std::string(",\"vsync\":") + (gowFrameGet(1) ? "true" : "false");
        if (gowFrameAvailable(2))
            json += std::string(",\"earlyFinish\":") + (gowFrameGet(2) ? "true" : "false");
        json += std::string(",\"internalResolution\":\"") + std::to_string(gowQualityGet(0) + 1) + "\"";
        static const char *const filters[3] = {"fsr", "bilinear", "pixel"};
        json += std::string(",\"filter\":\"") + filters[std::clamp(gowQualityGet(1), 0, 2)] + "\"";
        json += std::string(",\"sharpness\":") + std::to_string(gowQualityGet(2) / 100.0);
        json += std::string(",\"fxaa\":") + (gowQualityGet(3) ? "true" : "false");
        json += "}\n";
        const std::string &path = persistPath();
        const std::string temporary = path + ".tmp";
        if (std::FILE *f = std::fopen(temporary.c_str(), "wb"))
        {
            const bool ok = std::fwrite(json.data(), 1, json.size(), f) == json.size();
            std::fclose(f);
            std::remove(path.c_str()); // rename does not replace an existing file on every platform
            if (!ok || std::rename(temporary.c_str(), path.c_str()) != 0)
                std::fprintf(stderr, "[gow-persist] could not write %s\n", path.c_str());
            else if (std::getenv("GOW_PERSIST_DIAG"))
                std::fprintf(stderr, "[gow-persist] wrote %s: %s", path.c_str(), json.c_str());
        }
        else
            std::fprintf(stderr, "[gow-persist] could not open %s\n", temporary.c_str());
    }

    void persistThread()
    {
        for (;;)
        {
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
            if (g_persistDirty.load(std::memory_order_acquire) && persistNowMs() - g_persistTouchedMs.load(std::memory_order_relaxed) >= 400)
            {
                g_persistDirty.store(false, std::memory_order_release);
                persistWriteFile();
            }
        }
    }
}

void gowPersistTouch()
{
    if (persistPath().empty())
        return;
    g_persistTouchedMs.store(persistNowMs(), std::memory_order_relaxed);
    g_persistDirty.store(true, std::memory_order_release);
    static std::once_flag once;
    std::call_once(once, [] {
        std::thread(persistThread).detach();
        std::atexit([] {
            if (g_persistDirty.exchange(false))
                persistWriteFile();
        });
    });
}
