// GOW-Port: adaptado de Taylor N. Albarnaz / LightVelox (GPL-3.0).
// Origen: PS2Recomp sotc-port, ac9efa070638ad3b3accd284de6f898d5ab271d1.
#include "runtime/gs/gs_threaded_backend.h"
#include "runtime/gs/gs_hw_backend.h"
#include "runtime/gs/gs_shared_present.h"
#include "gs_inventory.h"
#include "ThreadPriority.h"
#include "ThreadNaming.h"

#include <atomic>
#include <cstring>
#include <algorithm>
#include <chrono>
#include <thread>
#include <bit>
#include <cstdlib>
#include <cstring>
#include <cstdio>
#include <emmintrin.h>
#include <xmmintrin.h>

namespace
{
    constexpr size_t kPublishBytes = 64u * 1024u;
    constexpr size_t kMaxPendingChunks = 256u;
    std::atomic<bool> s_enabledByDefault{false};

    bool discardDraws()
    {
        static const bool discard = []
        {
            const char *value = std::getenv("PS2X_GS_DISCARD_DRAWS");
            return value && *value && *value != '0';
        }();
        return discard;
    }

    struct LoadClutArgs
    {
        GSTex0Reg tex0;
        GSTexClutReg texclut;
    };

    struct WriteVramArgs
    {
        uint32_t psm;
        uint32_t base;
        uint32_t bw;
        uint32_t x;
        uint32_t y;
        uint32_t value;
    };

    template <typename T>
    T readAt(const uint8_t *data)
    {
        T value;
        std::memcpy(&value, data, sizeof(T));
        return value;
    }
}

void GSProducerLock::lockSlow()
{
    // A short spin first: the holder usually releases within a few hundred cycles.
    for (unsigned spins = 0u; spins < 256u; ++spins)
    {
        uint32_t expected = 0u;
        if (m_state.load(std::memory_order_relaxed) == 0u &&
            m_state.compare_exchange_strong(expected, 1u, std::memory_order_acquire, std::memory_order_relaxed))
            return;
        _mm_pause();
    }
    // Then announce a waiter (state 2) and block until an unlock wakes us. Taking the lock as 2 keeps the wake chain going
    // for any other waiter (one extra notify at most when there is none).
    while (m_state.exchange(2u, std::memory_order_acquire) != 0u)
        m_state.wait(2u, std::memory_order_relaxed);
}

GSThreadedBackend::GSThreadedBackend(std::unique_ptr<GSRasterBackend> inner)
    : m_inner(std::move(inner))
{
    const char *compact = std::getenv("PS2X_GS_COMPACT_QUEUE");
    m_compactQueue = !compact || *compact != '0';
    // GOW-Port: the default queue of 8 blocks of 64 KB (512 KB) was smaller than a heavy frame (≈ 7–13 MB of vertices): with the GL thread slightly
    // behind it filled up and Present() skipped the presentation (the window repeated the frame). Heavy combat (t≈125 s, 2.5×): frames
    // published per second 39 -> 53–55 (64 or 250 blocks) with the game at 53–55; at 3× 40 -> 55. With 64 blocks a batch at 3× stayed stuck at 45–48 after the fight
    m_queueChunkLimit = 200u;
    if (const char *limit = std::getenv("PS2X_GS_QUEUE_CHUNKS"))
        m_queueChunkLimit = static_cast<size_t>(std::clamp(std::atoi(limit), 1, static_cast<int>(kMaxPendingChunks)));
    m_open.reserve(kPublishBytes + 4096u);
    m_worker = std::thread([this]
                           { WorkerLoop(); });
}

GSThreadedBackend::~GSThreadedBackend()
{
    // GOW-Port: complete even the last block before closing the GL context.
    Drain();
    {
        std::lock_guard<std::mutex> lock(m_queueMutex);
        m_stop = true;
        m_pending.clear();
    }
    m_workCv.notify_all();
    if (m_worker.joinable())
        m_worker.join();
}

std::unique_ptr<GSRasterBackend> GSThreadedBackend::MakeDefault()
{
    // GOW-Port: the native render-target renderer is the only GS renderer (no VRAM-emulating rasteriser).
    return std::make_unique<GSThreadedBackend>(std::make_unique<GSHwBackend>());
}

void GSThreadedBackend::SetEnabledByDefault(bool enabled)
{
    s_enabledByDefault.store(enabled);
}


void GSThreadedBackend::Enqueue(Op op, const void *payload, uint32_t size, bool stateKnownSame)
{
    std::lock_guard<GSProducerLock> lock(m_producerMutex);
    const uint32_t csr = _mm_getcsr();
    if (csr != m_lastMxcsr)
    {
        m_lastMxcsr = csr;
        const uint32_t csrSize = sizeof(csr);
        uint8_t *at = m_open.extend(1u + sizeof(csrSize) + sizeof(csr));
        at[0] = static_cast<uint8_t>(Op::Mxcsr);
        std::memcpy(at + 1u, &csrSize, sizeof(csrSize));
        std::memcpy(at + 1u + sizeof(csrSize), &csr, sizeof(csr));
    }
    // GOW-Port: header (kind + length) and payload written with one capacity check, not two vector inserts.
    const auto append = [&](Op kind, const void *bytes, uint32_t length)
    {
        uint8_t *at = m_open.extend(1u + sizeof(length) + length);
        at[0] = static_cast<uint8_t>(kind);
        std::memcpy(at + 1u, &length, sizeof(length));
        if (length)
            std::memcpy(at + 1u + sizeof(length), bytes, length);
    };
    if (op == Op::Submit && m_compactQueue)
    {
        const auto &draw = *static_cast<const GSPrimitiveBatch *>(payload);
        // GOW-Port: field by field, not memcmp (padding bytes are unspecified: with clang no two states compared equal).
        // Identical bytes imply identical fields, so a memcmp that matches is conclusive and costs a few vector compares;
        // only a mismatch (a real change, or different padding) pays for the field-by-field comparison.
        // GOW-Port: stateKnownSame (SubmitSameState): the frontend saw no state register change since the previous draw, so the
        // bytes equal m_lastDrawState whenever that is valid; skip the comparison.
        if (!m_drawStateValid ||
            (!stateKnownSame && !(std::memcmp(&m_lastDrawState, &draw.state, sizeof(draw.state)) == 0 || m_lastDrawState == draw.state)))
        {
            std::memcpy(&m_lastDrawState, &draw.state, sizeof(draw.state));
            m_drawStateValid = true;
            append(Op::DrawState, &draw.state, sizeof(draw.state));
        }
        append(Op::Vertices, &draw, static_cast<uint32_t>(offsetof(GSPrimitiveBatch, state)));
    }
    else
        append(op, payload, size);
    if (m_open.size() >= kPublishBytes)
        PublishLocked();
}

void GSThreadedBackend::PublishLocked()
{
    if (m_open.empty())
        return;
    static const bool queueDiag = std::getenv("GOW_GS_QUEUE_DIAG") != nullptr;
    GSCommandBuffer next;
    {
        std::unique_lock<std::mutex> lock(m_queueMutex);
        const auto waitStart = queueDiag ? std::chrono::steady_clock::now() : std::chrono::steady_clock::time_point{};
        m_doneCv.wait(lock, [&]
                      { return m_stop || m_pending.size() < m_queueChunkLimit; });
        if (queueDiag)
        {
            const double waitMs = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - waitStart).count();
            if (waitMs >= 30.0)
                std::fprintf(stderr, "[gow-gs-queue] wait_ms=%.2f pending=%zu limit=%zu published=%llu completed=%llu\n",
                             waitMs, m_pending.size(), m_queueChunkLimit,
                             static_cast<unsigned long long>(m_published), static_cast<unsigned long long>(m_completed));
        }
        m_pending.push_back(std::move(m_open));
        ++m_published;
        if (!m_spare.empty())
        {
            next = std::move(m_spare.back());
            m_spare.pop_back();
        }
    }
    m_workCv.notify_one();
    next.clear();
    m_open = std::move(next);
    if (m_open.capacity() < kPublishBytes)
        m_open.reserve(kPublishBytes + 4096u);
}

void GSThreadedBackend::Drain() const
{
    auto *self = const_cast<GSThreadedBackend *>(this);
    uint64_t target = 0u;
    {
        std::lock_guard<GSProducerLock> lock(m_producerMutex);
        self->PublishLocked();
        std::lock_guard<std::mutex> queueLock(m_queueMutex);
        target = m_published;
    }
    std::unique_lock<std::mutex> lock(m_queueMutex);
    m_doneCv.wait(lock, [&]
                  { return m_completed >= target || m_stop; });
}

#if defined(_WIN32) && (defined(__x86_64__) || defined(_M_X64))
extern "C"
{
    __declspec(dllimport) void *__stdcall GetCurrentProcess(void);
    __declspec(dllimport) int __stdcall GetProcessAffinityMask(void *process, unsigned long long *processMask,
                                                              unsigned long long *systemMask);
}
#endif

namespace
{
    // Logical processors this process may run on (its affinity mask on Windows, so a run pinned with -Cores counts too).
    unsigned usableProcessorsForGl()
    {
#if defined(_WIN32) && (defined(__x86_64__) || defined(_M_X64))
        unsigned long long process = 0, system = 0;
        if (GetProcessAffinityMask(GetCurrentProcess(), &process, &system) && process != 0)
            return static_cast<unsigned>(std::popcount(process));
#endif
        return std::max(1u, std::thread::hardware_concurrency());
    }

    // The GL thread runs above normal priority only where the process has a core to spare. On 2 cores it preempted the EE
    // and the VU1 (measured at 60 FPS with the launcher's environment, 3 alternating pairs: 51.1 -> 52.2 FPS, worst 5 s
    // window 31.9 -> 37.2, frames over 50 ms 152 -> 113 with it off). PS2X_THREAD_PRIORITY=0/1 still forces it.
    bool raiseGlThread()
    {
        const char *value = std::getenv("PS2X_THREAD_PRIORITY");
        if (value && *value)
            return *value != '0';
        return usableProcessorsForGl() >= 4u;
    }
}

void GSThreadedBackend::WorkerLoop()
{
    ThreadNaming::SetCurrentThreadName("GSThread");
    if (raiseGlThread())
        ThreadPriority::RaiseCurrentThread(ThreadPriority::Level::AboveNormal);
    GSPrimitiveBatch draw;
    for (;;)
    {
        GSCommandBuffer chunk;
        {
            std::unique_lock<std::mutex> lock(m_queueMutex);
            m_workCv.wait(lock, [&]
                          { return m_stop || !m_pending.empty(); });
            if (m_stop)
            {
                m_inner.reset();
                return;
            }
            chunk = std::move(m_pending.front());
            m_pending.pop_front();
        }
        {
            std::lock_guard<std::mutex> exec(m_execMutex);
            Execute(chunk, draw);
        }
        {
            std::lock_guard<std::mutex> lock(m_queueMutex);
            ++m_completed;
            if (m_spare.size() < 64u)
                m_spare.push_back(std::move(chunk));
        }
        m_doneCv.notify_all();
    }
}

void GSThreadedBackend::Execute(const GSCommandBuffer &chunk, GSPrimitiveBatch &draw)
{
    const uint8_t *data = chunk.data();
    const size_t size = chunk.size();
    size_t offset = 0u;
    while (offset + 5u <= size)
    {
        const Op op = static_cast<Op>(data[offset]);
        const uint32_t length = readAt<uint32_t>(data + offset + 1u);
        const uint8_t *payload = data + offset + 5u;
        offset += 5u + length;
        switch (op)
        {
        case Op::Mxcsr:
            _mm_setcsr(readAt<uint32_t>(payload));
            break;
        case Op::Submit:
            if (!discardDraws())
                m_inner->Submit(readAt<GSPrimitiveBatch>(payload));
            break;
        case Op::LoadClut:
        {
            const LoadClutArgs args = readAt<LoadClutArgs>(payload);
            m_inner->LoadClut(args.tex0, args.texclut);
            break;
        }
        case Op::BeginTransfer:
            m_inner->BeginTransfer(readAt<GSTransferCommand>(payload));
            break;
        case Op::Upload:
            m_inner->UploadImage(payload, length);
            break;
        case Op::TextureFlush:
            m_inner->TextureFlush();
            break;
        case Op::WriteVram:
        {
            const WriteVramArgs a = readAt<WriteVramArgs>(payload);
            m_inner->WriteVram(a.psm, a.base, a.bw, a.x, a.y, a.value);
            break;
        }
        case Op::Sync:
            m_inner->Sync(readAt<GSSyncReason>(payload));
            break;
        case Op::DrawState:
            std::memcpy(&draw.state, payload, sizeof(draw.state));
            break;
        case Op::Vertices:
            std::memcpy(&draw, payload, offsetof(GSPrimitiveBatch, state));
            if (!discardDraws())
                m_inner->Submit(draw);
            break;
        case Op::Present:
            m_inner->Present(readAt<GSPresentationRequest>(payload));
            break;
        case Op::Call:
            (*readAt<const std::function<void()> *>(payload))();
            break;
        }
    }
}

void GSThreadedBackend::RunOnWorker(const std::function<void()> &function) const
{
    auto *self = const_cast<GSThreadedBackend *>(this);
    const std::function<void()> *pointer = &function;
    self->Enqueue(Op::Call, &pointer, sizeof(pointer));
    Drain();
}

void GSThreadedBackend::Initialize(uint8_t *vram, uint32_t vramSize)
{
    RunOnWorker([&] {
        m_inner->Initialize(vram, vramSize);
        m_sharedPresentation.store(m_inner->UsesSharedPresentation(), std::memory_order_release);
    });
}

void GSThreadedBackend::Reset()
{
    RunOnWorker([&] { m_inner->Reset(); });
}

void GSThreadedBackend::Submit(const GSPrimitiveBatch &batch)
{
    if (GsInventory::get().enabled())
        GsInventory::get().draw(batch);
    Enqueue(Op::Submit, &batch, sizeof(batch));
}

void GSThreadedBackend::SubmitSameState(const GSPrimitiveBatch &batch)
{
    if (GsInventory::get().enabled())
        GsInventory::get().draw(batch);
    Enqueue(Op::Submit, &batch, sizeof(batch), true);
}

void GSThreadedBackend::LoadClut(const GSTex0Reg &tex0, const GSTexClutReg &texclut)
{
    if (GsInventory::get().enabled())
        GsInventory::get().clut(tex0);
    const LoadClutArgs args{tex0, texclut};
    Enqueue(Op::LoadClut, &args, sizeof(args));
}

void GSThreadedBackend::BeginTransfer(const GSTransferCommand &command)
{
    if (GsInventory::get().enabled())
        GsInventory::get().transfer(command);
    Enqueue(Op::BeginTransfer, &command, sizeof(command));
}

void GSThreadedBackend::UploadImage(const uint8_t *data, uint32_t sizeBytes)
{
    Enqueue(Op::Upload, data, data ? sizeBytes : 0u);
}

void GSThreadedBackend::Flush()
{
    // GOW-Port: barrier for read/presentation and calls of the existing frontend.
    RunOnWorker([&] { m_inner->Flush(); });
}

void GSThreadedBackend::TextureFlush()
{
    Enqueue(Op::TextureFlush, nullptr, 0u);
}

// GOW-Port: GOW_GS_FINISH_ASYNC=1 (optional, off by default) queues FINISH on the GS thread
// without waiting or glFinish: the EE sees the FINISH bit right away. VRAM reads from the EE already synchronize
// by themselves (RunOnWorker) and the GS thread keeps the order of the commands.
static bool gowFinishAsync()
{
    static const bool on = [] { const char *v = std::getenv("GOW_GS_FINISH_ASYNC"); return v && v[0] == '1'; }();
    return on;
}

void GSThreadedBackend::Sync(GSSyncReason reason)
{
    if (reason == GSSyncReason::Finish && gowFinishAsync())
    {
        Enqueue(Op::Sync, &reason, sizeof(reason));
        return;
    }
    if (reason == GSSyncReason::Presentation && UsesSharedPresentation())
        return;
    // GOW-Port: FINISH is only published when the previous commands have finished.
    RunOnWorker([&] { m_inner->Sync(reason); });
}

PresentationFrame GSThreadedBackend::Present(const GSPresentationRequest &request)
{
    if (GsInventory::get().enabled())
        GsInventory::get().present();
    if (UsesSharedPresentation() && !request.forceReadback)
    {
        // GOW-Port: the window thread does not wait for a full queue. With the GPU behind (3x in heavy scenes) PublishLocked
        // blocked the presentation until the queue emptied: the game stayed at ~56 fps and the window dropped. If the queue is full
        // this presentation is skipped and the window repeats the last published frame. GOW_PRESENT_BLOCKING=1 waits again.
        // GOW-Port: explicitly interpret 0/false. Before, GOW_PRESENT_BLOCKING=0
        // still enabled the wait because the mere presence of the variable was considered true.
        static const bool blocking = [] {
            const char *value = std::getenv("GOW_PRESENT_BLOCKING");
            if (!value || !*value)
                return false;
            return std::strcmp(value, "0") != 0 && std::strcmp(value, "false") != 0 &&
                   std::strcmp(value, "FALSE") != 0;
        }();
        if (!blocking)
        {
            std::lock_guard<std::mutex> lock(m_queueMutex);
            if (m_pending.size() >= m_queueChunkLimit)
                return {};
        }
        Enqueue(Op::Present, &request, sizeof(request));
        std::lock_guard<GSProducerLock> lock(m_producerMutex);
        PublishLocked();
        return {};
    }
    PresentationFrame frame;
    RunOnWorker([&] { frame = m_inner->Present(request); });
    return frame;
}

bool GSThreadedBackend::ClearFramebuffer(const GSContext &context, uint32_t rgba)
{
    if (GsInventory::get().enabled())
        GsInventory::get().clear();
    bool result = false;
    RunOnWorker([&] { result = m_inner->ClearFramebuffer(context, rgba); });
    return result;
}

uint32_t GSThreadedBackend::ConsumeLocalToHostBytes(uint8_t *dst, uint32_t maxBytes)
{
    if (GsInventory::get().enabled())
        GsInventory::get().consumeL2H();
    uint32_t result = 0u;
    RunOnWorker([&] { result = m_inner->ConsumeLocalToHostBytes(dst, maxBytes); });
    return result;
}

uint32_t GSThreadedBackend::ReadVram(uint32_t psm, uint32_t base, uint32_t bw, uint32_t x, uint32_t y) const
{
    if (GsInventory::get().enabled())
        GsInventory::get().cpuRead();
    uint32_t result = 0u;
    RunOnWorker([&] { result = m_inner->ReadVram(psm, base, bw, x, y); });
    return result;
}

void GSThreadedBackend::WriteVram(uint32_t psm, uint32_t base, uint32_t bw, uint32_t x, uint32_t y, uint32_t value)
{
    if (GsInventory::get().enabled())
        GsInventory::get().cpuWrite();
    const WriteVramArgs args{psm, base, bw, x, y, value};
    Enqueue(Op::WriteVram, &args, sizeof(args));
}

void GSThreadedBackend::SnapshotVram(std::vector<uint8_t> &out) const
{
    if (GsInventory::get().enabled())
        GsInventory::get().snapshot();
    RunOnWorker([&] { m_inner->SnapshotVram(out); });
}

GSTransferSnapshot GSThreadedBackend::GetTransferSnapshot() const
{
    GSTransferSnapshot result;
    RunOnWorker([&] { result = m_inner->GetTransferSnapshot(); });
    return result;
}

bool GSThreadedBackend::ExportState(GSBackendState &out)
{
    bool result = false;
    RunOnWorker([&]
                {
        auto *access = dynamic_cast<GSBackendStateAccess *>(m_inner.get());
        result = access && access->ExportState(out); });
    return result;
}

bool GSThreadedBackend::ImportState(const GSBackendState &state)
{
    bool result = false;
    RunOnWorker([&]
                {
        auto *access = dynamic_cast<GSBackendStateAccess *>(m_inner.get());
        result = access && access->ImportState(state); });
    return result;
}
