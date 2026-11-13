// GOW-Port: performance OSD in the style of MSI Afterburner / RTSS. Each part has its own switch: PC_SHOW_FPS, PC_SHOW_GPU
// (GPU and video memory lines), PC_SHOW_CPU and PC_SHOW_RAM. It is included from gow_integration.cpp (no
// new file in CMake). It shows the GS FPS (new frames per second) large and, only if the system provides them, GPU (usage, VRAM,
// temperature, memory clock), CPU (usage, effective clock) and RAM. What cannot be read is not drawn (a value is never invented).
//
// Sources: PDH counters ("GPU Engine", "GPU Adapter Memory", "Processor Information") and D3DKMTQueryAdapterInfo (temperature and memory clock of
// the GL context's adapter). Everything is loaded dynamically (without linking pdh.lib) and the Windows functions are declared by hand
// because this file coexists with raylib.h, which clashes with windows.h. A thread reads them once per second.
#pragma once

#if defined(_WIN32)
extern "C"
{
    __declspec(dllimport) void *__stdcall LoadLibraryA(const char *);
    __declspec(dllimport) void *__stdcall GetProcAddress(void *, const char *);
    __declspec(dllimport) int __stdcall GetSystemTimes(void *idle, void *kernel, void *user);
    __declspec(dllimport) void *__stdcall wglGetCurrentDC(void);
    struct GowMemStatusEx
    {
        unsigned long length, memoryLoad;
        unsigned long long totalPhys, availPhys, totalPageFile, availPageFile, totalVirtual, availVirtual, availExtVirtual;
    };
    __declspec(dllimport) int __stdcall GlobalMemoryStatusEx(GowMemStatusEx *);
    // The process's own memory (working set = physical RAM the game occupies); K32GetProcessMemoryInfo is in kernel32 since Windows 7.
    struct GowProcMem
    {
        unsigned long cb, pageFaults;
        size_t peakWorkingSet, workingSet, quotaPeakPaged, quotaPaged, quotaPeakNonPaged, quotaNonPaged, pagefile, peakPagefile;
    };
    __declspec(dllimport) void *__stdcall GetCurrentProcess(void);
    __declspec(dllimport) int __stdcall K32GetProcessMemoryInfo(void *, GowProcMem *, unsigned long);
}
#endif

namespace gow_osd
{
    // -1 = el sistema no lo da.
    struct Metrics
    {
        std::atomic<int> cpuPercent{-1}, cpuMhz{-1}, ramMb{-1};
        std::atomic<int> gpuPercent{-1}, vramMb{-1}, gpuTempC{-1}, memMhz{-1};
    };
    Metrics g_metrics;

#if defined(_WIN32)
    struct PdhValue
    {
        unsigned long status;
        union { long longValue; double doubleValue; long long largeValue; const void *ptr; };
    };
    struct PdhItem
    {
        wchar_t *name;
        PdhValue value;
    };
    constexpr unsigned long kPdhFmtDouble = 0x200u;
    constexpr unsigned long kPdhFmtNoCap100 = 0x8000u;
    constexpr long kPdhMoreData = static_cast<long>(0x800007D2u);

    using PdhOpenQueryFn = long(__stdcall *)(const wchar_t *, unsigned long long, void **);
    using PdhAddCounterFn = long(__stdcall *)(void *, const wchar_t *, unsigned long long, void **);
    using PdhCollectFn = long(__stdcall *)(void *);
    using PdhArrayFn = long(__stdcall *)(void *, unsigned long, unsigned long *, unsigned long *, PdhItem *);
    using PdhValueFn = long(__stdcall *)(void *, unsigned long, unsigned long *, PdhValue *);
    using PdhCloseFn = long(__stdcall *)(void *);

    struct Pdh
    {
        PdhOpenQueryFn open = nullptr;
        PdhAddCounterFn add = nullptr;
        PdhCollectFn collect = nullptr;
        PdhArrayFn array = nullptr;
        PdhValueFn value = nullptr;
        PdhCloseFn close = nullptr;
        bool ok() const { return open && add && collect && array && value && close; }
    };

    struct KmtOpenFromHdc
    {
        void *hdc;
        unsigned adapter;
        struct { unsigned long low; long high; } luid;
        unsigned vidPnSource;
    };
    struct KmtQuery
    {
        unsigned adapter;
        int type;
        void *data;
        unsigned size;
    };
    struct KmtPerfData
    {
        unsigned physicalAdapterIndex;
        unsigned long long memoryFrequency, maxMemoryFrequency, maxMemoryFrequencyOC, memoryBandwidth, pcieBandwidth;
        unsigned long fanRpm, power, temperature;
        unsigned char powerStateOverride;
    };
    constexpr int kKmtAdapterPerfData = 62; // KMTQAITYPE_ADAPTERPERFDATA
    using KmtOpenFn = long(__stdcall *)(KmtOpenFromHdc *);
    using KmtQueryFn = long(__stdcall *)(KmtQuery *);

    std::atomic<unsigned> g_kmtAdapter{0};
    std::atomic<bool> g_kmtReady{false};

    // Called from the drawing thread (needs the current GL context): opens the adapter that draws the window.
    void openAdapterOnce()
    {
        static bool tried = false;
        if (tried)
            return;
        tried = true;
        void *gdi = LoadLibraryA("gdi32.dll");
        if (!gdi)
            return;
        const auto open = reinterpret_cast<KmtOpenFn>(GetProcAddress(gdi, "D3DKMTOpenAdapterFromHdc"));
        void *hdc = wglGetCurrentDC();
        if (!open || !hdc)
            return;
        KmtOpenFromHdc request{};
        request.hdc = hdc;
        if (open(&request) == 0 && request.adapter != 0u)
        {
            g_kmtAdapter.store(request.adapter);
            g_kmtReady.store(true);
        }
    }

    double sumInstances(const Pdh &pdh, void *counter, const wchar_t *mustContain, bool takeMax)
    {
        unsigned long bytes = 0, count = 0;
        if (pdh.array(counter, kPdhFmtDouble | kPdhFmtNoCap100, &bytes, &count, nullptr) != kPdhMoreData || bytes == 0)
            return -1.0;
        std::vector<unsigned char> buffer(bytes);
        auto *items = reinterpret_cast<PdhItem *>(buffer.data());
        if (pdh.array(counter, kPdhFmtDouble | kPdhFmtNoCap100, &bytes, &count, items) != 0)
            return -1.0;
        double total = 0.0;
        bool any = false;
        for (unsigned long i = 0; i < count; ++i)
        {
            if (mustContain && (!items[i].name || !std::wcsstr(items[i].name, mustContain)))
                continue;
            if (items[i].value.status != 0u && items[i].value.status != 1u)
                continue;
            any = true;
            total = takeMax ? std::max(total, items[i].value.doubleValue) : total + items[i].value.doubleValue;
        }
        return any ? total : -1.0;
    }

    void sampler()
    {
        void *pdhDll = LoadLibraryA("pdh.dll");
        Pdh pdh;
        if (pdhDll)
        {
            pdh.open = reinterpret_cast<PdhOpenQueryFn>(GetProcAddress(pdhDll, "PdhOpenQueryW"));
            pdh.add = reinterpret_cast<PdhAddCounterFn>(GetProcAddress(pdhDll, "PdhAddEnglishCounterW"));
            pdh.collect = reinterpret_cast<PdhCollectFn>(GetProcAddress(pdhDll, "PdhCollectQueryData"));
            pdh.array = reinterpret_cast<PdhArrayFn>(GetProcAddress(pdhDll, "PdhGetFormattedCounterArrayW"));
            pdh.value = reinterpret_cast<PdhValueFn>(GetProcAddress(pdhDll, "PdhGetFormattedCounterValue"));
            pdh.close = reinterpret_cast<PdhCloseFn>(GetProcAddress(pdhDll, "PdhCloseQuery"));
        }
        void *gdi = LoadLibraryA("gdi32.dll");
        const auto kmtQuery = gdi ? reinterpret_cast<KmtQueryFn>(GetProcAddress(gdi, "D3DKMTQueryAdapterInfo")) : nullptr;

        struct Query
        {
            void *query = nullptr, *gpu = nullptr, *vram = nullptr, *perf = nullptr, *freq = nullptr;
        } q;
        const auto build = [&]
        {
            if (q.query && pdh.ok())
                pdh.close(q.query);
            q = {};
            if (!pdh.ok() || pdh.open(nullptr, 0, &q.query) != 0)
            {
                q = {};
                return;
            }
            // The wildcard counters are recreated every few seconds: new processes and adapters appear.
            pdh.add(q.query, L"\\GPU Engine(*)\\Utilization Percentage", 0, &q.gpu);
            pdh.add(q.query, L"\\GPU Adapter Memory(*)\\Dedicated Usage", 0, &q.vram);
            pdh.add(q.query, L"\\Processor Information(_Total)\\% Processor Performance", 0, &q.perf);
            pdh.add(q.query, L"\\Processor Information(_Total)\\Processor Frequency", 0, &q.freq);
            pdh.collect(q.query);
        };
        build();

        unsigned long long lastIdle = 0, lastKernel = 0, lastUser = 0;
        const auto filetime = [](unsigned long long v) { return v; };
        int round = 0;
        for (;;)
        {
            std::this_thread::sleep_for(std::chrono::milliseconds(1000));
            if (++round % 15 == 0)
                build();
            // CPU: system time (the kernel time includes idle).
            unsigned long long idle = 0, kernel = 0, user = 0;
            if (GetSystemTimes(&idle, &kernel, &user))
            {
                if (lastKernel != 0)
                {
                    const unsigned long long total = (filetime(kernel) - lastKernel) + (filetime(user) - lastUser);
                    const unsigned long long busy = total - (filetime(idle) - lastIdle);
                    if (total > 0)
                        g_metrics.cpuPercent.store(static_cast<int>(100.0 * static_cast<double>(busy) / static_cast<double>(total) + 0.5));
                }
                lastIdle = idle;
                lastKernel = kernel;
                lastUser = user;
            }
            // RAM: the game process's (working set), not the whole system's.
            GowProcMem process{};
            process.cb = sizeof(process);
            if (K32GetProcessMemoryInfo(GetCurrentProcess(), &process, sizeof(process)))
                g_metrics.ramMb.store(static_cast<int>(process.workingSet / (1024ull * 1024ull)));
            if (q.query && pdh.ok() && pdh.collect(q.query) == 0)
            {
                // GPU: the 3D engine (the one the game uses); VRAM: the adapter with the most dedicated memory in use.
                const double gpu = q.gpu ? sumInstances(pdh, q.gpu, L"engtype_3D", false) : -1.0;
                g_metrics.gpuPercent.store(gpu < 0.0 ? -1 : static_cast<int>(std::min(100.0, gpu) + 0.5));
                const double vram = q.vram ? sumInstances(pdh, q.vram, nullptr, true) : -1.0;
                g_metrics.vramMb.store(vram < 0.0 ? -1 : static_cast<int>(vram / (1024.0 * 1024.0)));
                PdhValue perf{}, freq{};
                unsigned long type = 0;
                if (q.perf && q.freq && pdh.value(q.perf, kPdhFmtDouble | kPdhFmtNoCap100, &type, &perf) == 0 &&
                    pdh.value(q.freq, kPdhFmtDouble | kPdhFmtNoCap100, &type, &freq) == 0 && perf.doubleValue > 0.0 && freq.doubleValue > 0.0)
                    g_metrics.cpuMhz.store(static_cast<int>(freq.doubleValue * perf.doubleValue / 100.0 + 0.5));
            }
            // GPU: temperature and memory clock given by the driver (if it does not give them or they are implausible, they are not shown).
            if (g_kmtReady.load() && kmtQuery)
            {
                KmtPerfData perfData{};
                KmtQuery request{g_kmtAdapter.load(), kKmtAdapterPerfData, &perfData, static_cast<unsigned>(sizeof(perfData))};
                if (kmtQuery(&request) == 0)
                {
                    const double tempC = static_cast<double>(perfData.temperature) / 10.0;
                    g_metrics.gpuTempC.store(tempC >= 10.0 && tempC <= 120.0 ? static_cast<int>(tempC + 0.5) : -1);
                    const double mhz = static_cast<double>(perfData.memoryFrequency) / 1.0e6;
                    g_metrics.memMhz.store(mhz >= 100.0 && mhz <= 30000.0 ? static_cast<int>(mhz + 0.5) : -1);
                }
            }
        }
    }
#endif

    void start()
    {
#if defined(_WIN32)
        static std::once_flag once;
        std::call_once(once, [] { std::thread(sampler).detach(); });
#endif
    }

    struct Seg
    {
        std::string text;
        Color color;
        float scale;
    };

    constexpr float kSpacing = 0.07f; // letter spacing (RTSS's OSD uses a wide font)
    Font g_font{};
    bool g_fontLoaded = false, g_fontTried = false;

    void loadFont()
    {
        if (g_fontTried)
            return;
        g_fontTried = true;
        int glyphs[96 + 1];
        for (int i = 0; i < 95; ++i)
            glyphs[i] = 32 + i;
        glyphs[95] = 0xB0; // °
        for (const char *path : {"C:/Windows/Fonts/consolab.ttf", "C:/Windows/Fonts/arialbd.ttf", "C:/Windows/Fonts/segoeuib.ttf"})
        {
            if (!FileExists(path))
                continue;
            Font font = LoadFontEx(path, 56, glyphs, 96);
            if (font.texture.id != 0)
            {
                SetTextureFilter(font.texture, TEXTURE_FILTER_BILINEAR);
                g_font = font;
                g_fontLoaded = true;
                return;
            }
        }
    }

    float measure(const std::string &text, float size)
    {
        return g_fontLoaded ? MeasureTextEx(g_font, text.c_str(), size, size * kSpacing).x : static_cast<float>(MeasureText(text.c_str(), static_cast<int>(size)));
    }

    // Text with a dark outline (like RTSS's OSD), with the baseline shared between different sizes.
    void text(const std::string &value, float x, float baseline, float size, Color color)
    {
        const float y = baseline - size * 0.80f;
        const Color shadow{0, 0, 0, 235};
        const auto one = [&](float dx, float dy, Color c)
        {
            if (g_fontLoaded)
                DrawTextEx(g_font, value.c_str(), Vector2{std::round(x + dx), std::round(y + dy)}, size, size * kSpacing, c);
            else
                DrawText(value.c_str(), static_cast<int>(x + dx), static_cast<int>(y + dy), static_cast<int>(size), c);
        };
        one(-1.0f, 0.0f, shadow);
        one(1.0f, 0.0f, shadow);
        one(0.0f, -1.0f, shadow);
        one(0.0f, 1.0f, shadow);
        one(1.0f, 1.0f, shadow);
        one(0.0f, 0.0f, color);
    }

    // A numeric cell: number right-aligned in a fixed width + smaller unit. Returns the total width.
    float cell(float x, float baseline, float size, const std::string &number, const char *unit, float numberWidth, float unitWidth,
               Color valueColor, Color unitColor)
    {
        const float numberSize = size;
        const float unitSize = size * 0.72f;
        text(number, x + numberWidth - measure(number, numberSize), baseline, numberSize, valueColor);
        text(unit, x + numberWidth + size * 0.16f, baseline, unitSize, unitColor);
        return numberWidth + size * 0.16f + unitWidth;
    }

    // `gameFps` is the guest's own frame rate (entries to the game's frame function). The GS keeps producing frames even when
    // the game logic is behind, so the two numbers together tell what is happening: FPS high with GAME low = the picture is
    // repeating (the game stalled), both low = the machine cannot keep up. -1 = not measured (the line is not drawn).
    // GOW-Port: which parts of the OSD the player asked for (launcher, Performance tab).
    struct Show
    {
        bool fps = true, gpu = true, cpu = true, ram = true;
        bool any() const { return fps || gpu || cpu || ram; }
        bool system() const { return gpu || cpu || ram; }
    };

    // `gsFps`: new GS pictures that reached the screen per second, drawn as "GS" under FPS (-1 = not drawn).
    void draw(float x, float y, float h, int fps, int low, int windowHz = -1, int gameFps = -1, Show show = {}, int gsFps = -1)
    {
        // the sampler thread (PDH, D3DKMT) only runs when a system value is shown
        if (show.system())
            start();
#if defined(_WIN32)
        if (show.gpu)
            openAdapterOnce();
#endif
        loadFont();
        const float size = std::clamp(h / 42.0f, 14.0f, 34.0f);
        const float lineHeight = size * 1.20f;
        const Color green{70, 190, 90, 255}, blue{40, 150, 240, 255}, orange{255, 150, 30, 255}, unit{240, 130, 20, 255};
        const Color pink{200, 130, 130, 255}, white{255, 255, 255, 255};
        const float left = x + 10.0f;
        float baseline = y + 10.0f + size;

        // The widest label decides the column, so "GAME" never overlaps the numbers.
        const float labelWidth = measure(gameFps >= 0 ? "GAME" : "GPU", size) + size * 0.7f;
        const float nDigits4 = measure("88888", size), nDigits3 = measure("888", size); // 5 digits for MB / MHz
        const float uMhz = measure("MHz", size * 0.72f), uMb = measure("MB", size * 0.72f), uPct = measure("%", size * 0.72f),
                    uTemp = measure("\xC2\xB0" "C", size * 0.72f);
        const float colGap = size * 0.9f;
        const float col1 = left + labelWidth;                                  // temperatura / MB / %
        const float col2 = col1 + std::max(nDigits4 + uMb, nDigits3 + uTemp) + size * 0.16f + colGap; // uso
        const float col3 = col2 + nDigits3 + uPct + size * 0.16f + colGap;      // reloj

        const int gpu = show.gpu ? g_metrics.gpuPercent.load() : -1, vram = show.gpu ? g_metrics.vramMb.load() : -1;
        const int temp = show.gpu ? g_metrics.gpuTempC.load() : -1, memMhz = show.gpu ? g_metrics.memMhz.load() : -1;
        const int cpu = show.cpu ? g_metrics.cpuPercent.load() : -1, cpuMhz = show.cpu ? g_metrics.cpuMhz.load() : -1;
        const int ram = show.ram ? g_metrics.ramMb.load() : -1;

        // GPU: temperatura | uso
        if (gpu >= 0 || temp >= 0)
        {
            text("GPU", left, baseline, size, green);
            if (temp >= 0)
                cell(col1, baseline, size, std::to_string(temp), "\xC2\xB0" "C", nDigits3, uTemp, orange, unit);
            if (gpu >= 0)
                cell(col2, baseline, size, std::to_string(gpu), "%", nDigits3, uPct, orange, unit);
            baseline += lineHeight;
        }
        // MEM: VRAM | memory clock
        if (vram >= 0 || memMhz >= 0)
        {
            text("MEM", left, baseline, size, green);
            if (vram >= 0)
                cell(col1, baseline, size, std::to_string(vram), "MB", nDigits4, uMb, orange, unit);
            if (memMhz >= 0)
                cell(col2, baseline, size, std::to_string(memMhz), "MHz", nDigits4, uMhz, orange, unit);
            baseline += lineHeight;
        }
        // CPU: uso | reloj
        if (cpu >= 0 || cpuMhz >= 0)
        {
            text("CPU", left, baseline, size, blue);
            if (cpu >= 0)
                cell(col1, baseline, size, std::to_string(cpu), "%", nDigits3, uPct, orange, unit);
            if (cpuMhz >= 0)
                cell(col2, baseline, size, std::to_string(cpuMhz), "MHz", nDigits4, uMhz, orange, unit);
            baseline += lineHeight;
        }
        if (ram >= 0)
        {
            text("RAM", left, baseline, size, blue);
            cell(col1, baseline, size, std::to_string(ram), "MB", nDigits4, uMb, orange, unit);
            baseline += lineHeight;
        }
        (void)col3;
        if (!show.fps)
            return;
        // GS FPS: pink name and white number, the same size and in the same column as the other values.
        text("FPS", left, baseline, size, pink);
        cell(col1, baseline, size, std::to_string(fps), "", nDigits3, 0.0f, white, unit);
        // GOW-Port: GS = new pictures on screen per second; below FPS means the picture repeats and the player sees stutter.
        if (gsFps >= 0)
        {
            baseline += lineHeight;
            text("GS", left, baseline, size, pink);
            cell(col1, baseline, size, std::to_string(gsFps), "", nDigits3, 0.0f, white, unit);
        }
        // GOW-Port: GAME = the guest's frame rate. See the header comment: it is the number that tells the truth when the
        // picture is repeating.
        if (gameFps >= 0)
        {
            baseline += lineHeight;
            text("GAME", left, baseline, size, pink);
            cell(col1, baseline, size, std::to_string(gameFps), "", nDigits3, 0.0f, white, unit);
        }
        // GOW-Port: HZ = how often the window itself refreshes (optional; gow_integration.cpp no longer passes it). FPS is the
        // game's picture (new frames, up to the launcher's FPS mode: it drops when the picture hitches).
        if (windowHz >= 0)
        {
            baseline += lineHeight;
            text("HZ", left, baseline, size, pink);
            cell(col1, baseline, size, std::to_string(windowHz), "", nDigits3, 0.0f, white, unit);
        }
        // GOW-Port: the 1 % low line (parameter `low`) is computed but not drawn at the user's request; pass a value >= 0 from
        // gow_integration.cpp would enable it.
        (void)low;
    }
}
