#include "runtime/gs/gs_stage.h"
#include "runtime/ps2_perf.h"

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

#if defined(__x86_64__) || defined(_M_X64) || defined(__i386__) || defined(_M_IX86)
#include <immintrin.h>
#define GOW_STAGE_PAUSE() _mm_pause()
#else
#define GOW_STAGE_PAUSE() std::this_thread::yield()
#endif

// GOW-Port: two implementations of the stage queue behind the same API (gs_stage.h).
// - ring (default): a byte ring with one producer (the GS worker, the only caller of push) and one consumer (the stage
//   thread). In combat the worker sends hundreds of thousands of packets a second; the former mutex + vector queue made
//   both threads take the lock for every packet and woke the sleeping stage through the kernel for most of them. Packets
//   are now written into the ring and read in place; the mutex and condition variables are used only when a side really
//   sleeps, which the stage first avoids by spinning up to GOW_GS_STAGE_SPIN_US (default 50, 0: sleep at once). The bound
//   on queued bytes is kept (4 MB); a packet over 1 MB goes through a heap copy. afterBatch still runs after every batch.
//   Idea from gow-native-pc runtime patch 44 (there: combat +3.5% on top of its leaner VU1 code).
// - queue (GOW_GS_STAGE_RING=0): the former queue, for A/B comparisons.
namespace
{
    // PS2X_GS_WORKER_DIAG=1 also reports, every 5 s, how the worker and the stage waited on each other (idea from
    // gow-native-pc runtime patch 36): the worker blocked because the queue was full, the stage idle for lack of packets and
    // busy running them, and how often a push had to wake a sleeping stage (each one a kernel wake-up). Off, it costs one
    // check of a static bool per push and per batch.
    bool syncDiagEnabled()
    {
        static const bool on = [] { const char *v = std::getenv("PS2X_GS_WORKER_DIAG"); return v && v[0] == '1'; }();
        return on;
    }

    bool ringEnabled()
    {
        static const bool on = [] { const char *v = std::getenv("GOW_GS_STAGE_RING"); return !(v && v[0] == '0'); }();
        return on;
    }

    uint64_t spinMicroseconds()
    {
        static const uint64_t us = [] {
            const char *v = std::getenv("GOW_GS_STAGE_SPIN_US");
            return v ? static_cast<uint64_t>(std::strtoull(v, nullptr, 10)) : 50ull;
        }();
        return us;
    }

    uint64_t nowUs()
    {
        return static_cast<uint64_t>(
            std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now().time_since_epoch()).count());
    }

    struct SyncDiag
    {
        // Producer-side counters are atomics in the ring (no shared lock); the stage thread reads them for the report.
        std::atomic<uint64_t> packets{0}, bytes{0}, pushWaits{0}, pushWaitUs{0}, wakeups{0};
        uint64_t idleUs = 0, busyUs = 0; // stage thread only
        uint64_t windowStartUs = 0;
    };

    // Stage thread: prints and restarts the window once 5 s have passed.
    void reportSyncDiag(SyncDiag &d, uint64_t now)
    {
        if (d.windowStartUs == 0u)
        {
            d.windowStartUs = now;
            return;
        }
        if (now - d.windowStartUs < 5000000u)
            return;
        std::fprintf(stderr,
                     "[gs-stage:sync] window=%.2f packets=%llu mb=%.1f push_waits=%llu push_wait_ms=%.1f wakeups=%llu idle_ms=%.1f busy_ms=%.1f\n",
                     (now - d.windowStartUs) * 1e-6, static_cast<unsigned long long>(d.packets.exchange(0)),
                     d.bytes.exchange(0) / (1024.0 * 1024.0), static_cast<unsigned long long>(d.pushWaits.exchange(0)),
                     d.pushWaitUs.exchange(0) / 1000.0, static_cast<unsigned long long>(d.wakeups.exchange(0)), d.idleUs / 1000.0,
                     d.busyUs / 1000.0);
        d.idleUs = d.busyUs = 0;
        d.windowStartUs = now;
    }

    // Bound on queued bytes: enough for several XGKICK packets of a batch, small enough that the worker never
    // runs a whole frame ahead of the GS frontend.
    constexpr size_t kMaxQueuedBytes = 4u << 20;

    struct Common
    {
        std::thread thread;
        std::thread::id threadId;
        ps2_gs_stage::ProcessFn process;
        std::function<void()> afterBatch;
        SyncDiag diag;
    };

    // ---- queue (GOW_GS_STAGE_RING=0) -------------------------------------------------------------------------------------

    struct Packet
    {
        GifPathId path = GifPathId::Path1;
        std::vector<uint8_t> data;
    };

    struct QueueStage
    {
        std::mutex mutex;
        std::condition_variable work;   // stage waits for packets
        std::condition_variable space;  // producer waits for room
        std::condition_variable idle;   // waitIdle
        std::vector<Packet> queue; // GOW-Port: a vector keeps its capacity across swaps (MSVC deque allocates per Packet)
        std::vector<std::vector<uint8_t>> spare; // recycled buffers (avoid an allocation per packet)
        size_t queuedBytes = 0;
        bool busy = false;
        bool stop = false;
    };

    // ---- ring (default) --------------------------------------------------------------------------------------------------

    // A record: a 16-byte header, then the packet padded to 16 bytes. A packet over kInlineMax is a header alone that points
    // to a heap copy. A header with size kWrapMarker means "the rest of the ring is unused: continue at offset 0".
    constexpr size_t kRingBytes = kMaxQueuedBytes; // a power of two
    constexpr uint32_t kInlineMax = 1u << 20;
    constexpr uint32_t kWrapMarker = 0xFFFFFFFFu;
    struct RecordHeader
    {
        uint32_t size;
        uint32_t path;
        uint8_t *heap; // nullptr: the bytes follow the header
    };
    static_assert(sizeof(RecordHeader) <= 16u, "a ring record header takes 16 bytes");
    constexpr uint64_t pad16(uint64_t n) { return (n + 15u) & ~uint64_t{15u}; }

    struct RingStage
    {
        std::unique_ptr<uint8_t[]> ring;
        // Byte positions since start (never wrapped): the offset in the ring is position % kRingBytes.
        std::atomic<uint64_t> writePos{0}; // advanced by the producer after a record is complete
        std::atomic<uint64_t> readPos{0};  // advanced by the stage after a record has been processed
        std::atomic<bool> busy{false};     // the stage is between taking a batch and finishing afterBatch
        std::atomic<bool> stageSleeping{false};
        std::atomic<bool> producerSleeping{false};
        std::atomic<uint32_t> idleWaiters{0};
        std::atomic<bool> stop{false};
        std::mutex mutex; // only for the condition variables below
        std::condition_variable work, space, idle;
    };

    struct Stage
    {
        Common common;
        bool ring = true; // chosen at start()
        QueueStage queue;
        RingStage rs;
    };

    Stage &stage()
    {
        static Stage s;
        return s;
    }

    void queueLoop()
    {
        Stage &st = stage();
        QueueStage &s = st.queue;
        std::vector<Packet> batch;
        const bool diag = syncDiagEnabled();
        uint64_t busySince = 0;
        for (;;)
        {
            {
                std::unique_lock<std::mutex> lock(s.mutex);
                for (Packet &p : batch)
                {
                    p.data.clear();
                    s.spare.push_back(std::move(p.data));
                }
                batch.clear();
                s.busy = false;
                if (s.queue.empty())
                    s.idle.notify_all();
                uint64_t idleSince = 0;
                if (diag)
                {
                    idleSince = nowUs();
                    if (busySince != 0u)
                        st.common.diag.busyUs += idleSince - busySince;
                    reportSyncDiag(st.common.diag, idleSince);
                }
                s.work.wait(lock, [&s] { return s.stop || !s.queue.empty(); });
                if (s.queue.empty())
                    return;
                if (diag)
                {
                    busySince = nowUs();
                    st.common.diag.idleUs += busySince - idleSince;
                }
                batch.swap(s.queue);
                s.queuedBytes = 0;
                s.busy = true;
            }
            s.space.notify_all();
            for (const Packet &p : batch)
                st.common.process(p.path, p.data.data(), static_cast<uint32_t>(p.data.size()));
            if (st.common.afterBatch)
                st.common.afterBatch();
        }
    }

    void queuePush(GifPathId path, const uint8_t *data, uint32_t size)
    {
        Stage &st = stage();
        QueueStage &s = st.queue;
        // GOW-Port: the copy runs outside the mutex, which the stage thread also takes to swap the queue and recycle its
        // buffers, so a large packet no longer holds it up. The only producer is the GS worker (the call site in
        // ps2_runtime.cpp is guarded by onGsWorkerThread()), so the order of the queue is the order of the calls.
        Packet p;
        p.path = path;
        {
            std::lock_guard<std::mutex> lock(s.mutex);
            if (!s.spare.empty())
            {
                p.data = std::move(s.spare.back());
                s.spare.pop_back();
            }
        }
        p.data.assign(data, data + size);
        std::unique_lock<std::mutex> lock(s.mutex);
        const auto hasRoom = [&s, size] { return s.queuedBytes == 0 || s.queuedBytes + size <= kMaxQueuedBytes; };
        static const bool diag = syncDiagEnabled();
        if (diag && !hasRoom())
        {
            const uint64_t start = nowUs();
            s.space.wait(lock, hasRoom);
            st.common.diag.pushWaits.fetch_add(1, std::memory_order_relaxed);
            st.common.diag.pushWaitUs.fetch_add(nowUs() - start, std::memory_order_relaxed);
        }
        else
            s.space.wait(lock, hasRoom);
        s.queuedBytes += size;
        const bool wasEmpty = s.queue.empty();
        s.queue.push_back(std::move(p));
        if (diag)
        {
            st.common.diag.packets.fetch_add(1, std::memory_order_relaxed);
            st.common.diag.bytes.fetch_add(size, std::memory_order_relaxed);
            // The stage sleeps on `work` only when it found the queue empty; a push to an empty queue may wake it.
            if (wasEmpty && !s.busy)
                st.common.diag.wakeups.fetch_add(1, std::memory_order_relaxed);
        }
        lock.unlock();
        if (wasEmpty)
            s.work.notify_one();
    }

    bool queueWaitIdle()
    {
        QueueStage &s = stage().queue;
        std::unique_lock<std::mutex> lock(s.mutex);
        const auto isIdle = [&s] { return s.queue.empty() && !s.busy; };
        if (isIdle())
            return false;
        s.idle.wait(lock, isIdle);
        return true;
    }

    void queueStop()
    {
        QueueStage &s = stage().queue;
        {
            std::lock_guard<std::mutex> lock(s.mutex);
            s.stop = true;
        }
        s.work.notify_all();
    }

    // ---- ring ------------------------------------------------------------------------------------------------------------

    // Stage thread: after readPos moved, wake a producer waiting for room (seq_cst pairs with the producer's flag store).
    void ringReleased(RingStage &s)
    {
        if (s.producerSleeping.load(std::memory_order_seq_cst))
        {
            std::lock_guard<std::mutex> lock(s.mutex);
            s.space.notify_one();
        }
    }

    void ringLoop()
    {
        Stage &st = stage();
        RingStage &s = st.rs;
        const bool diag = syncDiagEnabled();
        const uint64_t spinUs = spinMicroseconds();
        uint64_t busySince = 0;
        for (;;)
        {
            // Idle: tell waitIdle callers, then wait for the next record (spin first, then sleep).
            s.busy.store(false, std::memory_order_seq_cst);
            if (s.idleWaiters.load(std::memory_order_seq_cst) != 0u)
            {
                std::lock_guard<std::mutex> lock(s.mutex);
                s.idle.notify_all();
            }
            uint64_t idleSince = 0;
            if (diag)
            {
                idleSince = nowUs();
                if (busySince != 0u)
                    st.common.diag.busyUs += idleSince - busySince;
                reportSyncDiag(st.common.diag, idleSince);
            }
            uint64_t read = s.readPos.load(std::memory_order_relaxed);
            uint64_t write = s.writePos.load(std::memory_order_acquire);
            if (write == read && spinUs != 0u)
            {
                const auto until = std::chrono::steady_clock::now() + std::chrono::microseconds(spinUs);
                unsigned n = 0;
                while ((write = s.writePos.load(std::memory_order_acquire)) == read && !s.stop.load(std::memory_order_relaxed))
                {
                    GOW_STAGE_PAUSE();
                    if ((++n & 63u) == 0u && std::chrono::steady_clock::now() >= until)
                        break;
                }
            }
            if (write == read)
            {
                std::unique_lock<std::mutex> lock(s.mutex);
                s.stageSleeping.store(true, std::memory_order_seq_cst);
                s.work.wait(lock, [&] {
                    return s.stop.load(std::memory_order_seq_cst) || s.writePos.load(std::memory_order_seq_cst) != read;
                });
                s.stageSleeping.store(false, std::memory_order_relaxed);
                write = s.writePos.load(std::memory_order_acquire);
                if (write == read)
                    return; // stopped with nothing left
            }
            s.busy.store(true, std::memory_order_seq_cst);
            if (diag)
            {
                busySince = nowUs();
                st.common.diag.idleUs += busySince - idleSince;
            }
            // A batch: every record complete when it was taken. readPos moves after each one, freeing its room.
            // GOW-Port: GOW_PERF_DIAG accounts the whole batch to the Gs bucket once; GS::processGIFPacket no longer
            // reads the clock twice per packet inside it.
            {
            ps2_perf::Scope batchPerf(ps2_perf::Bucket::Gs);
            while (read != write)
            {
                uint8_t *at = s.ring.get() + (read % kRingBytes);
                RecordHeader header;
                std::memcpy(&header, at, sizeof(header));
                uint64_t length = 16u;
                if (header.size == kWrapMarker)
                    length = kRingBytes - (read % kRingBytes);
                else if (header.heap != nullptr)
                {
                    st.common.process(static_cast<GifPathId>(header.path), header.heap, header.size);
                    delete[] header.heap;
                }
                else
                {
                    st.common.process(static_cast<GifPathId>(header.path), at + 16u, header.size);
                    length = 16u + pad16(header.size);
                }
                read += length;
                s.readPos.store(read, std::memory_order_seq_cst);
                ringReleased(s);
            }
            }
            if (st.common.afterBatch)
                st.common.afterBatch();
        }
    }

    void ringPush(GifPathId path, const uint8_t *data, uint32_t size)
    {
        Stage &st = stage();
        RingStage &s = st.rs;
        static const bool diag = syncDiagEnabled();
        const bool inlined = size <= kInlineMax;
        const uint64_t record = inlined ? 16u + pad16(size) : 16u;
        const uint64_t write = s.writePos.load(std::memory_order_relaxed);
        const uint64_t toEnd = kRingBytes - (write % kRingBytes);
        const bool wrap = toEnd < record; // the record does not fit before the end: mark the rest unused, start at 0
        const uint64_t need = record + (wrap ? toEnd : 0u);
        const auto hasRoom = [&] { return kRingBytes - (write - s.readPos.load(std::memory_order_seq_cst)) >= need; };
        if (!hasRoom())
        {
            const uint64_t start = diag ? nowUs() : 0u;
            std::unique_lock<std::mutex> lock(s.mutex);
            s.producerSleeping.store(true, std::memory_order_seq_cst);
            s.space.wait(lock, [&] { return hasRoom() || s.stop.load(std::memory_order_seq_cst); });
            s.producerSleeping.store(false, std::memory_order_relaxed);
            if (diag)
            {
                st.common.diag.pushWaits.fetch_add(1, std::memory_order_relaxed);
                st.common.diag.pushWaitUs.fetch_add(nowUs() - start, std::memory_order_relaxed);
            }
            if (!hasRoom())
                return; // stopping
        }
        uint64_t at = write;
        if (wrap)
        {
            RecordHeader marker{kWrapMarker, 0u, nullptr};
            std::memcpy(s.ring.get() + (at % kRingBytes), &marker, sizeof(marker));
            at += toEnd;
        }
        RecordHeader header{size, static_cast<uint32_t>(path), nullptr};
        uint8_t *slot = s.ring.get() + (at % kRingBytes);
        if (inlined)
            std::memcpy(slot + 16u, data, size);
        else
        {
            header.heap = new uint8_t[size];
            std::memcpy(header.heap, data, size);
        }
        std::memcpy(slot, &header, sizeof(header));
        s.writePos.store(write + need, std::memory_order_seq_cst);
        if (diag)
        {
            st.common.diag.packets.fetch_add(1, std::memory_order_relaxed);
            st.common.diag.bytes.fetch_add(size, std::memory_order_relaxed);
        }
        // seq_cst with the stage's flag store: either the stage sees the new writePos in its wait predicate, or this sees
        // it asleep and wakes it.
        if (s.stageSleeping.load(std::memory_order_seq_cst))
        {
            if (diag)
                st.common.diag.wakeups.fetch_add(1, std::memory_order_relaxed);
            std::lock_guard<std::mutex> lock(s.mutex);
            s.work.notify_one();
        }
    }

    bool ringWaitIdle()
    {
        RingStage &s = stage().rs;
        const auto isIdle = [&s] {
            return s.readPos.load(std::memory_order_seq_cst) == s.writePos.load(std::memory_order_seq_cst) &&
                   !s.busy.load(std::memory_order_seq_cst);
        };
        if (isIdle())
            return false;
        std::unique_lock<std::mutex> lock(s.mutex);
        s.idleWaiters.fetch_add(1, std::memory_order_seq_cst);
        s.idle.wait(lock, isIdle);
        s.idleWaiters.fetch_sub(1, std::memory_order_seq_cst);
        return true;
    }

    void ringStop()
    {
        RingStage &s = stage().rs;
        {
            std::lock_guard<std::mutex> lock(s.mutex);
            s.stop.store(true, std::memory_order_seq_cst);
        }
        s.work.notify_all();
        s.space.notify_all();
    }
}

namespace ps2_gs_stage
{
    void start(ProcessFn process, std::function<void()> afterBatch)
    {
        Stage &s = stage();
        if (s.common.thread.joinable())
            return;
        s.common.process = std::move(process);
        s.common.afterBatch = std::move(afterBatch);
        s.ring = ringEnabled();
        if (s.ring)
        {
            if (!s.rs.ring)
                s.rs.ring = std::make_unique<uint8_t[]>(kRingBytes);
            s.rs.stop.store(false, std::memory_order_seq_cst);
            s.common.thread = std::thread(ringLoop);
        }
        else
        {
            {
                std::lock_guard<std::mutex> lock(s.queue.mutex);
                s.queue.stop = false;
            }
            s.common.thread = std::thread(queueLoop);
        }
        s.common.threadId = s.common.thread.get_id();
        std::fprintf(stderr, "[gs-stage] GS frontend on its own thread, pipelined behind VU1 (%s; PS2X_GS_PIPELINE=0 disables)\n",
                     s.ring ? "lock-free ring, GOW_GS_STAGE_RING=0 for the former queue" : "mutex queue");
    }

    void stop()
    {
        Stage &s = stage();
        if (!s.common.thread.joinable())
            return;
        if (s.ring)
            ringStop();
        else
            queueStop();
        s.common.thread.join();
        s.common.threadId = {};
    }

    bool running()
    {
        return stage().common.thread.joinable();
    }

    bool onStageThread()
    {
        return running() && std::this_thread::get_id() == stage().common.threadId;
    }

    void push(GifPathId path, const uint8_t *data, uint32_t size)
    {
        if (stage().ring)
            ringPush(path, data, size);
        else
            queuePush(path, data, size);
    }

    bool waitIdle()
    {
        if (!running() || onStageThread())
            return false;
        return stage().ring ? ringWaitIdle() : queueWaitIdle();
    }
}
