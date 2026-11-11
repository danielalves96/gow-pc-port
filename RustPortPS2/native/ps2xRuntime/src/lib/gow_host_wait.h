// GOW-Port: timed condition-variable waits without the libc++ spin on Windows.
#pragma once

#include <chrono>
#include <condition_variable>
#include <mutex>
#include <thread>

// cv.wait_until(lock, deadline, pred), without the mutex churn. libc++ on Windows (llvm-mingw) waits on the SRW
// condition variable with the time left truncated to whole milliseconds: in the last millisecond the timeout is 0, the
// call returns at once and the predicate loop spins, taking and dropping the mutex 100-250 times per wait (measured).
// Here whole milliseconds are waited on the condition variable (a notify still wakes it) and the last fraction yields
// the CPU with the mutex released, checking the predicate every 50 us: ~14 lock round trips per wait, and the deadline
// is met as precisely as before (a few us). Sleeping that fraction instead (a high-resolution timer, ~0.4 ms late)
// cost 6 % of the frame rate on 4 cores: the EE has no slack there and every late wake is lost time. Other systems keep
// the standard wait.
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
