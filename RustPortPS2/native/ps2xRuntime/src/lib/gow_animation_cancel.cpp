// GOW-Port: Kratos animation cancelling in God of War (SCUS-97399), for more agile combat.
//
// How the game decides when the next move starts (see docs/dev/GAMEPLAY-HOOKS.md, "Animation canceling"):
//   *(Kratos+0x324)       Kratos's move controller. +0x58 current move id (0x50 standing,
//                         0x47-0x4A square combo, 0x20 triangle, 0x30/0x21/0x22 block, 0x201-0x208 dodge,
//                         0x64-0x68 hit received); +0x1AC state (0x18 attacking, 0x19 blocking).
//   controller+0x1C0      queue of transitions requested by button presses: {+0 pointer to entries, +4 u8 size,
//                         +5 u8 read (0xFF empty), +6 u8 write}; 8-byte entries {transition, threshold}.
//   0x2048F8              every frame pops the first transition off the queue once the current move's
//                         normalized progress reaches its threshold, and starts it with 0x2088E8(controller, transition).
// The game already cuts an attack short with block (L1) or dodge (right stick) on the next frame: those transitions
// arrive with threshold 0. What forces a wait is the threshold of the others: attacking out of a dodge waits for 50% of
// the roll (~0.6 s) and each hit of the square combo for 27-47% of the previous one. Here:
//   - Dodge: transitions requested during a dodge start at kDodgeThreshold of its progress at most.
//   - Combo: thresholds between attacks are multiplied by the GOW_CANCEL_COMBO factor (1 = as original).
//   - Input buffer: a square or triangle press the game didn't accept (e.g. releasing L1 the same
//     frame, or right before a move ends) is repeated a few times over kBufferWindow seconds.
// Only thresholds and button presses are changed: the transitions are the game's own. GOW_CANCEL=0 disables all of it.
//
// Investigation tools (local only; the dumps contain game RAM and are never uploaded):
//   GOW_CANCEL_DIAG=1                     logs every move that starts ([gow-animation-cancel] mov) and every transition
//                                           that enters Kratos's queue with its threshold ([gow-animation-cancel] cola).
//   GOW_CANCEL_SEGMENT="t:dur:start-end,..." saves those RAM regions (hex) on every pad read, into
//                                           cancel_segment.bin: double t, uint16 buttons, 6 padding bytes and the
//                                           regions in sequence. ps2-dev cancel-line displays them.
//   GOW_CANCEL_AXIS="t:dur:byte:value;..." forces a byte of the libpad2 buffer (2/3 right stick X/Y, 4/5 left).
//   GOW_CANCEL_DUMP="t1,t2,..."        saves the EE RAM to cancelar_ram_<t>.bin.
//   GOW_CANCEL_WATCH="addr:bytes:t:dur" (Linux x86-64) write-protects the EE RAM page at addr and
//                                           saves the RIP of every write to [addr, addr+bytes) (page fault and a
//                                           single-step with TF) to animation_cancel_writes.txt; nm on gow-2005 gives
//                                           the recompiled function.
// Times are counted from the first pad read, like GOW_FORCE_PAD.
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <string>
#include <utility>
#include <vector>
#if defined(__linux__) && defined(__x86_64__)
#include <csignal>
#include <link.h>
#include <sys/mman.h>
#include <ucontext.h>
#include <unistd.h>
#define GOW_CANCEL_WATCH 1
#endif

namespace
{
    constexpr uint32_t kPad2Read = 0x0027B9F0u;
    constexpr uint32_t kStartMove = 0x002088E8u;
    constexpr uint32_t kPlayerControl = 0x0029CD58u;
    constexpr uint16_t kSquare = 0x8000u, kTriangle = 0x1000u; // bytes 0-1 of the libpad2 buffer, active-low
    constexpr double kBufferWindow = 0.35;                     // seconds a press is remembered
    constexpr double kBufferDelay = 0.035;                     // before repeating it
    constexpr int kRepeats = 3;

    PS2Runtime::RecompiledFunction g_pad2Read = nullptr;
    PS2Runtime::RecompiledFunction g_startMove = nullptr;
    bool g_active = false, g_buffer = true, g_diag = false;
    float g_dodgeThreshold = 0.2f, g_comboFactor = 1.0f;

    double seconds()
    {
        static const auto start = std::chrono::steady_clock::now(); // first pad read
        return std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
    }

    uint32_t read32(const uint8_t *rdram, uint32_t a)
    {
        uint32_t v = 0u;
        std::memcpy(&v, rdram + (a & 0x1FFFFFFu), 4);
        return v;
    }

    bool valid(uint32_t a, uint32_t bytes)
    {
        a &= 0x1FFFFFFFu;
        return a >= 0x00100000u && a + bytes <= PS2_RAM_SIZE && (a & 3u) == 0u;
    }

    // Kratos's move controller (*(Kratos+0x324)) or 0.
    uint32_t kratosController(const uint8_t *rdram)
    {
        const uint32_t control = read32(rdram, kPlayerControl);
        const uint32_t kratos = valid(control, 4u) ? read32(rdram, control) : 0u;
        const uint32_t moves = valid(kratos, 0x360u) ? read32(rdram, kratos + 0x324u) : 0u;
        return valid(moves, 0x200u) ? moves : 0u;
    }

    struct Queue
    {
        uint32_t entries = 0u;
        uint8_t size = 0u, read = 0xFFu, write = 0u;
        bool empty() const { return read == 0xFFu || size == 0u || read >= size; }
        uint32_t count() const
        {
            if (empty())
                return 0u;
            const uint32_t n = (static_cast<uint32_t>(write) + size - read) % size;
            return n == 0u ? size : n;
        }
    };

    Queue readQueue(const uint8_t *rdram, uint32_t c)
    {
        Queue q;
        const uint32_t base = (c + 0x1C0u) & 0x1FFFFFFu;
        q.entries = read32(rdram, base);
        q.size = rdram[base + 4u];
        q.read = rdram[base + 5u];
        q.write = rdram[base + 6u];
        if (!valid(q.entries, 8u * q.size))
            q.read = 0xFFu;
        return q;
    }

    bool isDodge(uint32_t id) { return id >= 0x200u && id <= 0x20Fu; }
    bool isAttacking(uint32_t state) { return (state & 0x19u) == 0x18u; }

    // ---- Queue thresholds ----
    void adjustQueue(uint8_t *rdram, uint32_t c)
    {
        static uint32_t scaled[8] = {};
        const Queue q = readQueue(rdram, c);
        if (q.empty())
        {
            std::memset(scaled, 0, sizeof(scaled));
            return;
        }
        const uint32_t id = read32(rdram, c + 0x58u);
        const bool combo = g_comboFactor < 1.0f && isAttacking(read32(rdram, c + 0x1ACu));
        for (uint32_t k = 0, i = q.read; k < q.count(); ++k, i = (i + 1u) % q.size)
        {
            const uint32_t e = (q.entries + 8u * i) & 0x1FFFFFFu;
            float threshold = 0.0f;
            std::memcpy(&threshold, rdram + e + 4u, 4);
            float next = threshold;
            if (isDodge(id))
                next = std::min(threshold, g_dodgeThreshold);
            else if (combo && threshold > 0.0f)
            {
                // Each entry is scaled only once: the key is the transition and its slot in the queue.
                const uint32_t key = read32(rdram, e) ^ (i << 28);
                if (scaled[i & 7u] != key)
                {
                    scaled[i & 7u] = key;
                    next = threshold * g_comboFactor;
                }
            }
            if (next != threshold)
                std::memcpy(rdram + e + 4u, &next, 4);
        }
    }

    // ---- Input buffer ----
    struct Pending
    {
        bool active = false;
        double since = 0.0, last = 0.0;
        int repeats = 0;
        bool releasing = false; // frame where the button is forced released before pressing it again
        uint32_t move = 0u;
    };
    Pending g_pending[2];
    uint16_t g_previous = 0u; // actually pressed buttons in the previous read (1 = pressed)

    // The game accepted the press: there is a transition queued, one requested at +0x44, or another attacking move started.
    bool accepted(const uint8_t *rdram, uint32_t c, const Pending &p)
    {
        return !readQueue(rdram, c).empty() || read32(rdram, c + 0x44u) != 0u ||
               (isAttacking(read32(rdram, c + 0x1ACu)) && read32(rdram, c + 0x50u) != p.move);
    }

    void applyBuffer(uint8_t *rdram, uint32_t buffer, uint32_t c, double t)
    {
        uint16_t buttons = 0xFFFFu;
        std::memcpy(&buttons, rdram + buffer, 2);
        const uint16_t pressed = static_cast<uint16_t>(~buttons);
        const uint16_t masks[2] = {kSquare, kTriangle};
        for (int b = 0; b < 2; ++b)
        {
            const uint16_t m = masks[b];
            Pending &p = g_pending[b];
            if ((pressed & m) && !(g_previous & m))
            {
                p = {};
                if (g_diag)
                    std::fprintf(stderr, "[gow-animation-cancel] buffer t=%.3f press 0x%X (move 0x%X)\n", t, m, read32(rdram, c + 0x58u));
                p.active = true;
                p.since = p.last = t;
                p.move = read32(rdram, c + 0x50u);
                continue;
            }
            if (!p.active)
                continue;
            if (p.releasing)
            {
                buttons = static_cast<uint16_t>(buttons & ~m); // press it again: the game sees a new press
                if (g_diag)
                    std::fprintf(stderr, "[gow-animation-cancel] buffer t=%.3f repeat 0x%X (move 0x%X)\n", t, m, read32(rdram, c + 0x58u));
                p.releasing = false;
                p.last = t;
                ++p.repeats;
                continue;
            }
            if (accepted(rdram, c, p) || t - p.since > kBufferWindow || p.repeats >= kRepeats)
            {
                if (g_diag)
                    std::fprintf(stderr, "[gow-animation-cancel] buffer t=%.3f end 0x%X (%s, %d repeats)\n", t, m,
                                 accepted(rdram, c, p) ? "accepted" : "expired", p.repeats);
                p.active = false;
                continue;
            }
            if (t - p.last >= kBufferDelay)
            {
                buttons = static_cast<uint16_t>(buttons | m); // one frame released
                p.releasing = true;
            }
        }
        g_previous = pressed;
        std::memcpy(rdram + buffer, &buttons, 2);
    }

    // ---- Investigation tools ----
    std::vector<double> g_dumps;
    struct Axis
    {
        double t, dur;
        unsigned byte, value;
    };
    std::vector<Axis> g_axes;
    std::FILE *g_segment = nullptr;
    double g_segmentStart = 0.0, g_segmentEnd = 0.0;
    std::vector<std::pair<uint32_t, uint32_t>> g_regions;

    void tools(uint8_t *rdram, uint32_t buffer, double t)
    {
        for (double &v : g_dumps)
            if (v > 0.0 && t >= v)
            {
                char name[64];
                std::snprintf(name, sizeof(name), "cancelar_ram_%.2f.bin", v);
                if (std::FILE *f = std::fopen(name, "wb"))
                {
                    std::fwrite(rdram, 1, PS2_RAM_SIZE, f);
                    std::fclose(f);
                }
                v = -1.0;
            }
        for (const Axis &e : g_axes)
            if (t >= e.t && t < e.t + e.dur && e.byte < 18u)
                rdram[buffer + e.byte] = static_cast<uint8_t>(e.value);
        if (g_segment && t >= g_segmentStart)
        {
            if (t > g_segmentEnd)
            {
                std::fclose(g_segment);
                g_segment = nullptr;
                std::fprintf(stderr, "[gow-animation-cancel] segment closed\n");
            }
            else
            {
                uint16_t buttons = 0xFFFFu;
                const uint8_t padding[6] = {};
                std::memcpy(&buttons, rdram + buffer, 2);
                std::fwrite(&t, sizeof(t), 1, g_segment);
                std::fwrite(&buttons, 2, 1, g_segment);
                std::fwrite(padding, 1, sizeof(padding), g_segment);
                for (const auto &[a, b] : g_regions)
                    std::fwrite(rdram + a, 1, b - a, g_segment);
            }
        }
    }

    void diagQueue(const uint8_t *rdram, uint32_t c)
    {
        static uint32_t last = 0u;
        const Queue q = readQueue(rdram, c);
        if (q.empty())
        {
            last = 0u;
            return;
        }
        const uint32_t e = q.entries + 8u * q.read;
        const uint32_t reg = read32(rdram, e);
        float threshold = 0.0f;
        std::memcpy(&threshold, rdram + ((e + 4u) & 0x1FFFFFFu), 4);
        const uint32_t key = reg ^ (static_cast<uint32_t>(q.read) << 24) ^ (static_cast<uint32_t>(q.write) << 16);
        if (key == last)
            return;
        last = key;
        std::fprintf(stderr, "[gow-animation-cancel] queue t=%.3f mov=0x%X reg=0x%X threshold=%.2f reads=%u writes=%u\n", seconds(),
                     read32(rdram, c + 0x58u), reg, threshold, q.read, q.write);
    }

#ifdef GOW_CANCEL_WATCH
    struct Write
    {
        double t;
        uint64_t rip;
        uint32_t addr;
        uint32_t value;
    };
    constexpr size_t kMaxWrites = 4096u;
    Write g_writes[kMaxWrites];
    std::atomic<size_t> g_writeCount{0};
    uint8_t *g_rdram = nullptr;
    uint32_t g_watchAddr = 0u, g_watchBytes = 0u;
    double g_watchStart = 0.0, g_watchEnd = 0.0;
    uintptr_t g_page = 0u;
    size_t g_pageSize = 4096u;
    std::atomic<int> g_watchState{0}; // 0 not started, 1 protecting, 2 finished
    struct sigaction g_prevSegv{}, g_prevTrap{};
    thread_local bool t_stepPending = false;
    thread_local uint64_t t_rip = 0u;
    thread_local uint32_t t_addr = 0u;

    void segv(int sig, siginfo_t *info, void *context)
    {
        const uintptr_t a = reinterpret_cast<uintptr_t>(info->si_addr);
        if (g_page != 0u && a >= g_page && a < g_page + g_pageSize)
        {
            auto *uc = static_cast<ucontext_t *>(context);
            mprotect(reinterpret_cast<void *>(g_page), g_pageSize, PROT_READ | PROT_WRITE);
            t_stepPending = true;
            t_rip = static_cast<uint64_t>(uc->uc_mcontext.gregs[REG_RIP]);
            t_addr = static_cast<uint32_t>(a - reinterpret_cast<uintptr_t>(g_rdram));
            uc->uc_mcontext.gregs[REG_EFL] |= 0x100; // TF: single-step then SIGTRAP
            return;
        }
        if (g_prevSegv.sa_flags & SA_SIGINFO)
        {
            if (g_prevSegv.sa_sigaction)
                g_prevSegv.sa_sigaction(sig, info, context);
            return;
        }
        if (g_prevSegv.sa_handler == SIG_DFL || g_prevSegv.sa_handler == SIG_IGN)
        {
            signal(SIGSEGV, SIG_DFL); // the instruction repeats and the process ends as usual
            return;
        }
        g_prevSegv.sa_handler(sig);
    }

    void trap(int sig, siginfo_t *info, void *context)
    {
        if (!t_stepPending)
        {
            if ((g_prevTrap.sa_flags & SA_SIGINFO) && g_prevTrap.sa_sigaction)
                g_prevTrap.sa_sigaction(sig, info, context);
            return;
        }
        auto *uc = static_cast<ucontext_t *>(context);
        uc->uc_mcontext.gregs[REG_EFL] &= ~0x100;
        t_stepPending = false;
        const uint32_t d = t_addr & ~3u;
        if (d + 4u > g_watchAddr && d < g_watchAddr + g_watchBytes)
        {
            const size_t i = g_writeCount.fetch_add(1u);
            if (i < kMaxWrites)
            {
                uint32_t v = 0u;
                std::memcpy(&v, g_rdram + d, 4);
                g_writes[i] = {seconds(), t_rip, t_addr, v};
            }
        }
        if (g_watchState.load() == 1)
            mprotect(reinterpret_cast<void *>(g_page), g_pageSize, PROT_READ);
    }

    uintptr_t executableBase()
    {
        uintptr_t base = 0u;
        dl_iterate_phdr([](dl_phdr_info *i, size_t, void *data) {
            if (i->dlpi_name == nullptr || i->dlpi_name[0] == '\0')
            {
                *static_cast<uintptr_t *>(data) = static_cast<uintptr_t>(i->dlpi_addr);
                return 1;
            }
            return 0;
        }, &base);
        return base;
    }

    // From the pad read (EE thread): starts and ends the watch.
    void watch(uint8_t *rdram, double t)
    {
        const int state = g_watchState.load();
        if (g_watchBytes == 0u || state == 2)
            return;
        if (state == 0 && t >= g_watchStart)
        {
            g_rdram = rdram;
            g_pageSize = static_cast<size_t>(sysconf(_SC_PAGESIZE));
            g_page = reinterpret_cast<uintptr_t>(rdram + g_watchAddr) & ~(g_pageSize - 1u);
            struct sigaction sa{};
            sa.sa_sigaction = &segv;
            sa.sa_flags = SA_SIGINFO | SA_NODEFER;
            sigemptyset(&sa.sa_mask);
            sigaction(SIGSEGV, &sa, &g_prevSegv);
            sa.sa_sigaction = &trap;
            sigaction(SIGTRAP, &sa, &g_prevTrap);
            g_watchState.store(1);
            mprotect(reinterpret_cast<void *>(g_page), g_pageSize, PROT_READ);
            std::fprintf(stderr, "[gow-animation-cancel] watching 0x%X (%u bytes)\n", g_watchAddr, g_watchBytes);
        }
        else if (state == 1 && t > g_watchEnd)
        {
            g_watchState.store(2);
            mprotect(reinterpret_cast<void *>(g_page), g_pageSize, PROT_READ | PROT_WRITE);
            const size_t n = std::min(g_writeCount.load(), kMaxWrites);
            if (std::FILE *f = std::fopen("animation_cancel_writes.txt", "w"))
            {
                std::fprintf(f, "base %llx\n", static_cast<unsigned long long>(executableBase()));
                for (size_t i = 0; i < n; ++i)
                    std::fprintf(f, "%.3f %llx %x %x\n", g_writes[i].t, static_cast<unsigned long long>(g_writes[i].rip),
                                 g_writes[i].addr, g_writes[i].value);
                std::fclose(f);
            }
            std::fprintf(stderr, "[gow-animation-cancel] %zu writes in animation_cancel_writes.txt\n", n);
        }
    }
#endif

    // 0x2088E8(controller, transition): starts a move (only with GOW_CANCEL_DIAG).
    void startMove(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc == kStartMove)
        {
            const uint32_t control = GPR_U32(ctx, 4);
            std::fprintf(stderr, "[gow-animation-cancel] mov t=%.3f ra=0x%X ctl=0x%X reg=0x%X (before 0x%X)\n", seconds(),
                         GPR_U32(ctx, 31), control, GPR_U32(ctx, 5),
                         valid(control, 0x60u) ? read32(rdram, control + 0x58u) : 0u);
        }
        g_startMove(rdram, ctx, runtime);
    }

    void pad2Read(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t buffer = GPR_U32(ctx, 5) & 0x01FFFFFFu;
        g_pad2Read(rdram, ctx, runtime);
        const double t = seconds();
#ifdef GOW_CANCEL_WATCH
        watch(rdram, t);
#endif
        if (GPR_U32(ctx, 2) != 18u || buffer + 18u > PS2_RAM_SIZE)
            return;
        tools(rdram, buffer, t);
        const uint32_t c = kratosController(rdram);
        if (!c)
            return;
        if (g_active)
        {
            adjustQueue(rdram, c);
            if (g_buffer)
            {
                // GOW-Port: the buffer is for gameplay only. In the pause menu (game state 12) nothing "accepts" a Triangle, so
                // it kept pressing it again and one tap went back several menu levels (Options, pause menu, game).
                if (read32(rdram, 0x0029E560u) == 11u)
                    applyBuffer(rdram, buffer, c, t);
                else
                {
                    g_pending[0] = {};
                    g_pending[1] = {};
                    uint16_t buttons = 0xFFFFu;
                    std::memcpy(&buttons, rdram + buffer, 2);
                    g_previous = static_cast<uint16_t>(~buttons);
                }
            }
        }
        if (g_diag)
            diagQueue(rdram, c);
    }

    float readFactor(const char *name, float fallback)
    {
        const char *v = std::getenv(name);
        if (!v || !*v)
            return fallback;
        const float f = std::strtof(v, nullptr);
        return f >= 0.0f && f <= 1.0f ? f : fallback;
    }
}

void gowAnimationCancelInstall(PS2Runtime &runtime, const std::string &elfPath)
{
    if (std::filesystem::path(elfPath).filename() != "SCUS_973.99" || !runtime.hasFunction(kPad2Read))
        return;
    const char *active = std::getenv("GOW_CANCEL");
    g_active = !(active && active[0] == '0');
    g_dodgeThreshold = readFactor("GOW_CANCEL_EVASION", g_dodgeThreshold);
    g_comboFactor = readFactor("GOW_CANCEL_COMBO", g_comboFactor);
    if (const char *v = std::getenv("GOW_CANCEL_BUFFER"); v && v[0] == '0')
        g_buffer = false;

    if (const char *v = std::getenv("GOW_CANCEL_DUMP"))
        for (const char *p = v; *p;)
        {
            char *end = nullptr;
            const double t = std::strtod(p, &end);
            if (end == p)
                break;
            g_dumps.push_back(t);
            p = *end == ',' ? end + 1 : end;
        }
    if (const char *v = std::getenv("GOW_CANCEL_AXIS"))
    {
        const std::string list(v);
        size_t pos = 0u;
        while (pos < list.size())
        {
            size_t end = list.find(';', pos);
            if (end == std::string::npos)
                end = list.size();
            Axis e{};
            if (std::sscanf(list.substr(pos, end - pos).c_str(), "%lf:%lf:%u:%u", &e.t, &e.dur, &e.byte, &e.value) == 4)
                g_axes.push_back(e);
            pos = end + 1u;
        }
    }
    if (const char *v = std::getenv("GOW_CANCEL_SEGMENT"))
    {
        char list[512] = {};
        if (std::sscanf(v, "%lf:%lf:%511s", &g_segmentStart, &g_segmentEnd, list) == 3)
        {
            g_segmentEnd += g_segmentStart;
            for (char *tok = std::strtok(list, ","); tok; tok = std::strtok(nullptr, ","))
            {
                unsigned a = 0u, b = 0u;
                if (std::sscanf(tok, "%x-%x", &a, &b) == 2 && a < b && b <= PS2_RAM_SIZE)
                    g_regions.emplace_back(a & ~3u, b & ~3u);
            }
            if (!g_regions.empty())
                g_segment = std::fopen("cancel_segment.bin", "wb");
        }
    }
#ifdef GOW_CANCEL_WATCH
    if (const char *v = std::getenv("GOW_CANCEL_WATCH"))
    {
        unsigned addr = 0u, bytes = 0u;
        if (std::sscanf(v, "%x:%u:%lf:%lf", &addr, &bytes, &g_watchStart, &g_watchEnd) == 4 && addr + bytes <= PS2_RAM_SIZE)
        {
            g_watchAddr = addr;
            g_watchBytes = bytes;
            g_watchEnd += g_watchStart;
        }
    }
#endif
    if (const char *v = std::getenv("GOW_CANCEL_DIAG"); v && v[0] == '1' && runtime.hasFunction(kStartMove))
    {
        g_diag = true;
        g_startMove = runtime.lookupFunction(kStartMove);
        runtime.replaceFunction(kStartMove, &startMove);
    }
    // Chains with whatever wrapper was already there (gow_targeting.cpp).
    g_pad2Read = runtime.lookupFunction(kPad2Read);
    runtime.replaceFunction(kPad2Read, &pad2Read);
    if (g_active)
        std::cerr << "[gow-animation-cancel] agile cancelling: dodge at " << g_dodgeThreshold << ", combo x" << g_comboFactor
                  << (g_buffer ? ", input buffer" : "") << std::endl;
}
