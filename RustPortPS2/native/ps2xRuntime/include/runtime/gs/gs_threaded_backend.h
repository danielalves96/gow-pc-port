// GOW-Port: adaptado de Taylor N. Albarnaz / LightVelox (GPL-3.0).
// Origen: PS2Recomp sotc-port, ac9efa070638ad3b3accd284de6f898d5ab271d1.
#pragma once

#include "runtime/gs/gs_backend.h"
#include "runtime/gs/gs_backend_state.h"

#include <condition_variable>
#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <functional>
#include <memory>
#include <mutex>
#include <new>
#include <thread>
#include <vector>

// GOW-Port: byte buffer of the command queue. std::vector<uint8_t>::insert (one per header and one per payload of every
// command) showed up in the stage thread profile; this appends with one capacity check and a memcpy, and does not zero
// what it grows. Only the producer appends; the chunk is then moved to the worker, which reads data()/size().
class GSCommandBuffer
{
public:
    GSCommandBuffer() = default;
    GSCommandBuffer(const GSCommandBuffer &) = delete;
    GSCommandBuffer &operator=(const GSCommandBuffer &) = delete;
    GSCommandBuffer(GSCommandBuffer &&other) noexcept
        : m_data(other.m_data), m_size(other.m_size), m_capacity(other.m_capacity)
    {
        other.m_data = nullptr;
        other.m_size = other.m_capacity = 0u;
    }
    GSCommandBuffer &operator=(GSCommandBuffer &&other) noexcept
    {
        if (this != &other)
        {
            std::free(m_data);
            m_data = other.m_data;
            m_size = other.m_size;
            m_capacity = other.m_capacity;
            other.m_data = nullptr;
            other.m_size = other.m_capacity = 0u;
        }
        return *this;
    }
    ~GSCommandBuffer() { std::free(m_data); }

    const uint8_t *data() const { return m_data; }
    size_t size() const { return m_size; }
    size_t capacity() const { return m_capacity; }
    bool empty() const { return m_size == 0u; }
    void clear() { m_size = 0u; }
    void reserve(size_t capacity)
    {
        if (capacity > m_capacity)
            grow(capacity);
    }
    // Room for `extra` more bytes; returns where they go (the caller fills them right away).
    uint8_t *extend(size_t extra)
    {
        if (m_capacity - m_size < extra)
            grow(m_size + extra > m_capacity * 2u ? m_size + extra : m_capacity * 2u);
        uint8_t *at = m_data + m_size;
        m_size += extra;
        return at;
    }

private:
    void grow(size_t capacity)
    {
        auto *bytes = static_cast<uint8_t *>(std::realloc(m_data, capacity));
        if (!bytes)
            throw std::bad_alloc();
        m_data = bytes;
        m_capacity = capacity;
    }

    uint8_t *m_data = nullptr;
    size_t m_size = 0u;
    size_t m_capacity = 0u;
};

// GOW-Port: lock of the producer side of the queue. The GS stage thread takes it for every command (hundreds of thousands
// a second in combat) and nobody else does in the steady state, so an uncontended std::mutex (an SRW lock: two calls into
// ntdll with an interlocked operation each) was ~7 % of that thread. One interlocked operation to take it and a plain store
// to release. Contended (another thread issuing a barrier while the stage fills the queue, or the holder waiting for room
// in PublishLocked) it spins briefly and then blocks on the state word (std::atomic wait): state 0 free, 1 held, 2 held with
// waiters (the classic futex mutex). Only an unlock that finds waiters wakes one, so the uncontended path stays one interlocked
// operation each way. It must not poll with sleeps: on Windows a sleep lasts at least ~1 ms (15.6 ms without a raised timer
// resolution), and a waiter that polls that way stalled whole frames in heavy scenes.
class GSProducerLock
{
public:
    void lock()
    {
        uint32_t expected = 0u;
        if (m_state.compare_exchange_strong(expected, 1u, std::memory_order_acquire, std::memory_order_relaxed))
            return;
        lockSlow();
    }
    void unlock()
    {
        if (m_state.exchange(0u, std::memory_order_release) == 2u)
            m_state.notify_one();
    }

private:
    void lockSlow();
    std::atomic<uint32_t> m_state{0u};
};

class GSThreadedBackend final : public GSRasterBackend, public GSBackendStateAccess
{
public:
    explicit GSThreadedBackend(std::unique_ptr<GSRasterBackend> inner);
    ~GSThreadedBackend() override;

    static std::unique_ptr<GSRasterBackend> MakeDefault();
    static void SetEnabledByDefault(bool enabled);

    void Initialize(uint8_t *vram, uint32_t vramSize) override;
    void Reset() override;

    void Submit(const GSPrimitiveBatch &batch) override;
    void LoadClut(const GSTex0Reg &tex0, const GSTexClutReg &texclut) override;

    void BeginTransfer(const GSTransferCommand &command) override;
    void UploadImage(const uint8_t *data, uint32_t sizeBytes) override;

    void Flush() override;
    void TextureFlush() override;
    void Sync(GSSyncReason reason) override;
    PresentationFrame Present(const GSPresentationRequest &request) override;
    bool UsesSharedPresentation() const override { return m_sharedPresentation.load(std::memory_order_acquire); }

    bool ClearFramebuffer(const GSContext &context, uint32_t rgba) override;
    uint32_t ConsumeLocalToHostBytes(uint8_t *dst, uint32_t maxBytes) override;

    uint32_t ReadVram(uint32_t psm, uint32_t base, uint32_t bw, uint32_t x, uint32_t y) const override;
    void WriteVram(uint32_t psm, uint32_t base, uint32_t bw, uint32_t x, uint32_t y, uint32_t value) override;
    void SnapshotVram(std::vector<uint8_t> &out) const override;
    GSTransferSnapshot GetTransferSnapshot() const override;

    bool ExportState(GSBackendState &out) override;
    bool ImportState(const GSBackendState &state) override;

    GSRasterBackend &Inner() { return *m_inner; }

private:
    enum class Op : uint8_t
    {
        Mxcsr,
        Submit,
        LoadClut,
        BeginTransfer,
        Upload,
        TextureFlush,
        WriteVram,
        Sync,
        Present,
        DrawState,
        Vertices,
        Call,
    };

    void Enqueue(Op op, const void *payload, uint32_t size);
    void PublishLocked();
    void Drain() const;
    void RunOnWorker(const std::function<void()> &function) const;
    void WorkerLoop();
    void Execute(const GSCommandBuffer &chunk, GSPrimitiveBatch &draw);

    std::unique_ptr<GSRasterBackend> m_inner;
    // GOW-Port: published by the worker after Initialize, without querying GL from the host.
    std::atomic<bool> m_sharedPresentation{false};

    // GOW-Port: own cache line; the flag above is read by other threads and must not drag the producer fields with it.
    alignas(64) mutable GSProducerLock m_producerMutex;
    GSCommandBuffer m_open;
    uint32_t m_lastMxcsr = UINT32_MAX;
    GSDrawState m_lastDrawState{};
    bool m_drawStateValid = false;
    bool m_compactQueue = true;
    size_t m_queueChunkLimit = 8u;

    mutable std::mutex m_queueMutex;
    mutable std::condition_variable m_workCv;
    mutable std::condition_variable m_doneCv;
    std::deque<GSCommandBuffer> m_pending;
    std::vector<GSCommandBuffer> m_spare;
    uint64_t m_published = 0u;
    uint64_t m_completed = 0u;
    bool m_stop = false;

    mutable std::mutex m_execMutex;
    std::thread m_worker;
};
