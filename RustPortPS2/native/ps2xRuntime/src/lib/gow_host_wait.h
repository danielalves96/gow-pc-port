// GOW-Port: timed condition-variable waits without the libc++ spin on Windows.
#pragma once

#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <thread>

#if defined(_WIN32)
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <bit>
#include <cstdint>

// How the last fraction of a millisecond before a deadline is waited. Yielding meets the deadline within a few us but
// keeps the thread runnable: where the process has a core to spare (4 or more usable processors) that is the right trade
// (sleeping it on a high-resolution timer, ~0.4 ms late, cost 6 % of the frame rate on 4 cores). With fewer processors the
// yield loop takes the CPU the VU1 and GS threads need, so the fraction is slept on a high-resolution waitable timer.
// GOW_HOST_WAIT_FRACTION=yield|sleep overrides the choice (A/B).
inline bool gowHostWaitYields()
{
    static const bool yields = [] {
        if (const char *v = std::getenv("GOW_HOST_WAIT_FRACTION"))
            return std::strcmp(v, "sleep") != 0;
        DWORD_PTR process = 0, system = 0;
        unsigned count = std::max(1u, std::thread::hardware_concurrency());
        if (GetProcessAffinityMask(GetCurrentProcess(), &process, &system) && process != 0)
            count = static_cast<unsigned>(std::popcount(static_cast<uint64_t>(process)));
        return count >= 4u;
    }();
    return yields;
}

// Sleeps `left` on a high-resolution waitable timer (Windows 10 1803+); falls back to yielding once when it is missing.
inline void gowHighResSleep(std::chrono::nanoseconds left)
{
    static thread_local HANDLE timer = CreateWaitableTimerExW(nullptr, nullptr, 0x00000002 /* CREATE_WAITABLE_TIMER_HIGH_RESOLUTION */,
                                                              TIMER_ALL_ACCESS);
    if (!timer)
    {
        std::this_thread::yield();
        return;
    }
    LARGE_INTEGER due;
    due.QuadPart = -static_cast<LONGLONG>(std::max<int64_t>(1, left.count() / 100)); // relative, 100 ns units
    if (SetWaitableTimer(timer, &due, 0, nullptr, nullptr, FALSE))
        WaitForSingleObject(timer, INFINITE);
}
#endif

// cv.wait_until(lock, deadline, pred), without the mutex churn. libc++ on Windows (llvm-mingw) waits on the SRW
// condition variable with the time left truncated to whole milliseconds: in the last millisecond the timeout is 0, the
// call returns at once and the predicate loop spins, taking and dropping the mutex 100-250 times per wait (measured).
// Here whole milliseconds are waited on the condition variable (a notify still wakes it) and the last fraction yields
// the CPU with the mutex released, checking the predicate every 50 us (or sleeps it, see gowHostWaitYields): ~14 lock
// round trips per wait, and the deadline is met as precisely as before (a few us) when yielding. Other systems keep the
// standard wait.
template <class Predicate>
bool gowWaitUntil(std::condition_variable &cv, std::unique_lock<std::mutex> &lock,
                  std::chrono::steady_clock::time_point deadline, Predicate pred)
{
#if defined(_WIN32)
    using namespace std::chrono;
    while (!pred())
    {
        const auto now = steady_clock::now();
        if (now >= deadline)
            return pred();
        const auto left = deadline - now;
        if (left >= milliseconds(1))
        {
            cv.wait_for(lock, floor<milliseconds>(left));
            continue;
        }
        lock.unlock();
        if (!gowHostWaitYields())
        {
            gowHighResSleep(duration_cast<nanoseconds>(left));
            lock.lock();
            continue;
        }
        const auto check = now + microseconds(50);
        for (;;)
        {
            std::this_thread::yield();
            const auto t = steady_clock::now();
            if (t >= deadline || t >= check)
                break;
        }
        lock.lock();
    }
    return true;
#else
    return cv.wait_until(lock, deadline, pred);
#endif
}
