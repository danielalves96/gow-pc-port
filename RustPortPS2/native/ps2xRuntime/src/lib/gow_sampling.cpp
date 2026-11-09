// GOW-Port: sampling profiler for the process on Windows.
//   GOW_SAMPLING=<file>          enables sampling and is the output file
//   GOW_SAMPLING_FROM=<s>       seconds after startup before sampling begins (default 0)
//   GOW_SAMPLING_SEC=<s>         sampling duration (default 20)
//   GOW_SAMPLING_US=<µs>         interval between samples (default 2000)
// NOTE: env var names and the output file format below are a contract with
// ps2-dev sampling-win and must be changed together.
// Each interval briefly suspends every thread in the process, reads its instruction pointer and
// resumes it. Nothing is allocated or locked while a thread is suspended (it could hold the heap lock).
// On completion it writes the file read by ps2-dev sampling-win together with
// gow-2005.map (CMake option PS2X_MAP).
#include <algorithm>
#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <string>
#include <thread>
#include <utility>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <io.h>
#include <psapi.h>
#include <tlhelp32.h>
#include <mmsystem.h>
#else
#include <dlfcn.h>
#include <ucontext.h>
#include <unistd.h>
#endif

#ifdef _WIN32

namespace
{
    struct SampledThread
    {
        HANDLE handle = nullptr;
        uint64_t samples = 0;
        uint64_t startCpuMs = 0; // CPU ms at the start
        std::string name;
    };

    uint64_t cpuMs(HANDLE h)
    {
        FILETIME creationTime, exitTime, kernelTime, userTime;
        if (!GetThreadTimes(h, &creationTime, &exitTime, &kernelTime, &userTime))
            return 0;
        const auto a = [](const FILETIME &f) { return (static_cast<uint64_t>(f.dwHighDateTime) << 32) | f.dwLowDateTime; };
        return (a(kernelTime) + a(userTime)) / 10000u; // 100 ns units
    }

    std::string threadName(HANDLE h)
    {
        using Fn = HRESULT(WINAPI *)(HANDLE, PWSTR *);
        static const Fn getDescription = reinterpret_cast<Fn>(GetProcAddress(GetModuleHandleA("kernel32.dll"), "GetThreadDescription"));
        if (!getDescription)
            return {};
        PWSTR text = nullptr;
        std::string result;
        if (SUCCEEDED(getDescription(h, &text)) && text)
        {
            char buffer[128] = {};
            WideCharToMultiByte(CP_UTF8, 0, text, -1, buffer, sizeof(buffer) - 1, nullptr, nullptr);
            result = buffer;
            LocalFree(text);
        }
        return result;
    }

    // NOTE: output file format tokens ('base', 'duration', 'interval_us', 'module', 'thread', 'cpu_ms',
    // 'samples', 's') are parsed by ps2-dev sampling-win and must be changed together.
    void body(std::string path, double delay, double seconds, int intervalUs)
    {
        using Clock = std::chrono::steady_clock;
        std::this_thread::sleep_for(std::chrono::duration<double>(delay));
        timeBeginPeriod(1);
        const DWORD self = GetCurrentThreadId();
        const DWORD pid = GetCurrentProcessId();
        std::map<DWORD, SampledThread> threads;
        std::map<std::pair<DWORD, uint64_t>, uint64_t> counts;

        const auto refresh = [&]
        {
            const HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if (snapshot == INVALID_HANDLE_VALUE)
                return;
            THREADENTRY32 te{};
            te.dwSize = sizeof(te);
            for (BOOL ok = Thread32First(snapshot, &te); ok; ok = Thread32Next(snapshot, &te))
            {
                if (te.th32OwnerProcessID != pid || te.th32ThreadID == self || threads.count(te.th32ThreadID))
                    continue;
                const HANDLE h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE, te.th32ThreadID);
                if (!h)
                    continue;
                SampledThread &thread = threads[te.th32ThreadID];
                thread.handle = h;
                thread.startCpuMs = cpuMs(h);
                thread.name = threadName(h);
            }
            CloseHandle(snapshot);
        };

        refresh();
        const auto start = Clock::now();
        const auto end = start + std::chrono::duration_cast<Clock::duration>(std::chrono::duration<double>(seconds));
        auto next = start;
        auto lastRefresh = start;
        while (Clock::now() < end)
        {
            for (auto &entry : threads)
            {
                SampledThread &thread = entry.second;
                if (SuspendThread(thread.handle) == static_cast<DWORD>(-1))
                    continue;
                CONTEXT context{};
                context.ContextFlags = CONTEXT_CONTROL;
                const bool valid = GetThreadContext(thread.handle, &context) != 0;
                const uint64_t rip = valid ? context.Rip : 0;
                ResumeThread(thread.handle);
                if (valid) // outside the suspension: safe to allocate memory here
                {
                    ++counts[{entry.first, rip}];
                    ++thread.samples;
                }
            }
            if (Clock::now() - lastRefresh > std::chrono::milliseconds(500))
            {
                refresh();
                lastRefresh = Clock::now();
            }
            next += std::chrono::microseconds(intervalUs);
            std::this_thread::sleep_until(next);
        }
        timeEndPeriod(1);

        std::FILE *f = std::fopen(path.c_str(), "w");
        if (!f)
            return;
        std::fprintf(f, "base %llx\n", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(GetModuleHandleA(nullptr))));
        std::fprintf(f, "duration %.3f interval_us %d\n", seconds, intervalUs);
        HMODULE modules[1024];
        DWORD neededBytes = 0;
        if (EnumProcessModules(GetCurrentProcess(), modules, sizeof(modules), &neededBytes))
            for (DWORD i = 0; i < neededBytes / sizeof(HMODULE) && i < 1024; ++i)
            {
                MODULEINFO info{};
                char moduleName[MAX_PATH] = {};
                if (GetModuleInformation(GetCurrentProcess(), modules[i], &info, sizeof(info)) && GetModuleFileNameExA(GetCurrentProcess(), modules[i], moduleName, MAX_PATH))
                    std::fprintf(f, "module %llx %lx %s\n", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(info.lpBaseOfDll)),
                                 static_cast<unsigned long>(info.SizeOfImage), moduleName);
            }
        for (auto &entry : threads)
        {
            const uint64_t cpu = cpuMs(entry.second.handle) - entry.second.startCpuMs;
            std::fprintf(f, "thread %lu cpu_ms %llu samples %llu %s\n", static_cast<unsigned long>(entry.first), static_cast<unsigned long long>(cpu),
                         static_cast<unsigned long long>(entry.second.samples), entry.second.name.c_str());
        }
        for (const auto &c : counts)
            std::fprintf(f, "s %lu %llx %llu\n", static_cast<unsigned long>(c.first.first), static_cast<unsigned long long>(c.first.second),
                         static_cast<unsigned long long>(c.second));
        std::fclose(f);
        std::fprintf(stderr, "[sampling] %s written\n", path.c_str());
        for (auto &entry : threads)
            CloseHandle(entry.second.handle);
    }
}

#ifdef _WIN32
void gowSamplingBurst(const char *path, double seconds, int intervalUs)
{
    // GOW-Port: synchronous sampling (called by the hang watchdog of gow_integration.cpp)
    body(std::string(path), 0.0, seconds, intervalUs < 200 ? 200 : intervalUs);
}
#endif

void gowSamplingStart()
{
    const char *path = std::getenv("GOW_SAMPLING");
    if (!path || !path[0])
        return;
    static std::atomic<bool> started{false};
    if (started.exchange(true))
        return;
    const auto number = [](const char *name, double fallback)
    {
        const char *v = std::getenv(name);
        return v && v[0] ? std::atof(v) : fallback;
    };
    const double delay = number("GOW_SAMPLING_FROM", 0.0);
    const double seconds = number("GOW_SAMPLING_SEC", 20.0);
    const int us = static_cast<int>(number("GOW_SAMPLING_US", 2000.0));
    std::thread(body, std::string(path), delay, seconds, us < 200 ? 200 : us).detach();
}

// GOW-Port: crash report. A fault or an abort used to end the process with nothing in last.log (issue #35: the log just
// stops, and the launcher only knows the session was "interrupted"). On the first unhandled exception (or abort) one
// [gow-crash] line says what happened and where ("gow-2005+0x..." is found in gow-2005.map), after the last guest
// branches. The process then ends as before, with the same exit code, so the launcher still classifies the failure.
void gowDumpBranchRing(const char *why); // ps2_runtime.cpp

namespace
{
    HANDLE g_crashLog = INVALID_HANDLE_VALUE; // the file the launcher opened for stderr (not the log pipe)
    std::atomic<bool> g_crashed{false};

    void crashWrite(const char *text)
    {
        if (g_crashLog == INVALID_HANDLE_VALUE || g_crashLog == nullptr)
            return;
        DWORD written = 0;
        WriteFile(g_crashLog, text, static_cast<DWORD>(std::strlen(text)), &written, nullptr);
    }

    void describeAddress(uintptr_t address, char *out, size_t size)
    {
        HMODULE module = nullptr;
        char path[MAX_PATH] = {};
        if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                               reinterpret_cast<LPCSTR>(address), &module) &&
            module && GetModuleFileNameA(module, path, MAX_PATH))
        {
            const char *name = std::strrchr(path, '\\');
            std::snprintf(out, size, "%s+0x%llx", name ? name + 1 : path,
                          static_cast<unsigned long long>(address - reinterpret_cast<uintptr_t>(module)));
        }
        else
            std::snprintf(out, size, "0x%llx", static_cast<unsigned long long>(address));
    }

    void crashReport(const char *line)
    {
        // The guest branches go through stderr on a helper thread: the crashed thread may hold the stderr lock, so the
        // helper is not waited for more than half a second.
        if (HANDLE helper = CreateThread(nullptr, 0, [](LPVOID) -> DWORD { gowDumpBranchRing("crash"); std::fflush(stderr); return 0; },
                                         nullptr, 0, nullptr))
        {
            WaitForSingleObject(helper, 500);
            CloseHandle(helper);
        }
        Sleep(300); // the log writer thread empties the pipe into the file first, so the line below comes last
        crashWrite(line);
    }

    LONG WINAPI crashFilter(EXCEPTION_POINTERS *info)
    {
        if (info && info->ExceptionRecord && !g_crashed.exchange(true))
        {
            const EXCEPTION_RECORD *record = info->ExceptionRecord;
            char where[192];
            describeAddress(reinterpret_cast<uintptr_t>(record->ExceptionAddress), where, sizeof(where));
            char access[64] = "";
            if (record->ExceptionCode == EXCEPTION_ACCESS_VIOLATION && record->NumberParameters >= 2)
            {
                const ULONG_PTR kind = record->ExceptionInformation[0];
                std::snprintf(access, sizeof(access), " %s 0x%llx", kind == 0 ? "reading" : kind == 1 ? "writing" : "executing",
                              static_cast<unsigned long long>(record->ExceptionInformation[1]));
            }
            char line[384];
            std::snprintf(line, sizeof(line), "[gow-crash] exception 0x%08lX at %s%s, thread %lu\n",
                          static_cast<unsigned long>(record->ExceptionCode), where, access, static_cast<unsigned long>(GetCurrentThreadId()));
            crashReport(line);
        }
        return EXCEPTION_CONTINUE_SEARCH;
    }

    void crashAbort(int)
    {
        if (!g_crashed.exchange(true))
            crashReport("[gow-crash] abort (a failed check or an unhandled C++ exception; [terminate] lines above say which)\n");
    }
}

void gowCrashHandlerInstall(int logFd)
{
    static std::atomic<bool> installed{false};
    if (installed.exchange(true))
        return;
    g_crashLog = logFd >= 0 ? reinterpret_cast<HANDLE>(_get_osfhandle(logFd)) : GetStdHandle(STD_ERROR_HANDLE);
    SetUnhandledExceptionFilter(crashFilter);
    std::signal(SIGABRT, crashAbort);
}
#else
void gowSamplingStart() {}
void gowSamplingBurst(const char *, double, int) {}

// GOW-Port: crash report on Linux (see the Windows version above): one [gow-crash] line with the signal and the host
// address ("gow-2005+0x..."), then the last guest branches; the signal is raised again with its default action.
void gowDumpBranchRing(const char *why); // ps2_runtime.cpp

namespace
{
    int g_crashFd = 2;
    std::atomic<bool> g_crashed{false};

    void crashSignal(int signalNumber, siginfo_t *info, void *context)
    {
        if (!g_crashed.exchange(true))
        {
            uintptr_t pc = 0;
#if defined(__x86_64__) && defined(REG_RIP)
            if (context)
                pc = static_cast<uintptr_t>(static_cast<ucontext_t *>(context)->uc_mcontext.gregs[REG_RIP]);
#else
            (void)context;
#endif
            char where[256];
            Dl_info module{};
            if (pc && dladdr(reinterpret_cast<void *>(pc), &module) && module.dli_fname)
            {
                const char *name = std::strrchr(module.dli_fname, '/');
                std::snprintf(where, sizeof(where), "%s+0x%llx", name ? name + 1 : module.dli_fname,
                              static_cast<unsigned long long>(pc - reinterpret_cast<uintptr_t>(module.dli_fbase)));
            }
            else
                std::snprintf(where, sizeof(where), "0x%llx", static_cast<unsigned long long>(pc));
            char line[384];
            const int length = std::snprintf(line, sizeof(line), "[gow-crash] signal %d at %s, address 0x%llx\n", signalNumber, where,
                                             static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(info ? info->si_addr : nullptr)));
            if (length > 0)
                (void)!write(g_crashFd, line, static_cast<size_t>(std::min<int>(length, sizeof(line) - 1)));
            gowDumpBranchRing("crash");
        }
        std::signal(signalNumber, SIG_DFL);
        std::raise(signalNumber);
    }
}

void gowCrashHandlerInstall(int logFd)
{
    static std::atomic<bool> installed{false};
    if (installed.exchange(true))
        return;
    g_crashFd = logFd >= 0 ? logFd : 2;
    struct sigaction action{};
    action.sa_sigaction = crashSignal;
    action.sa_flags = SA_SIGINFO | SA_RESETHAND;
    sigemptyset(&action.sa_mask);
    for (const int signalNumber : {SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGABRT})
        sigaction(signalNumber, &action, nullptr);
}
#endif
