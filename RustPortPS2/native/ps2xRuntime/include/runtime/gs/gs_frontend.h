#ifndef PS2_GS_FRONTEND_H
#define PS2_GS_FRONTEND_H

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <atomic>
#include <mutex>
#include <thread>
#include <vector>

#include "runtime/gs/gs_backend.h"
#include "runtime/gs/ps2_gif_arbiter.h" // GOW-Port: keep the origin of each GIF stream.

// GOW-Port: a recursive lock on top of std::mutex for GSFrontend::m_stateMutex. libc++ on Windows (llvm-mingw) makes
// std::recursive_mutex a CRITICAL_SECTION: ~58 ns per lock contended by two threads, against ~31 ns for std::mutex (an
// SRW lock), and the EE and the GS worker take this one for every GIF packet. Same semantics: the owning thread may lock
// it again.
class GSStateMutex
{
public:
    void lock()
    {
        const std::thread::id self = std::this_thread::get_id();
        if (m_owner.load(std::memory_order_relaxed) == self)
        {
            ++m_depth;
            return;
        }
        m_mutex.lock();
        m_owner.store(self, std::memory_order_relaxed);
        m_depth = 1u;
    }

    bool try_lock()
    {
        const std::thread::id self = std::this_thread::get_id();
        if (m_owner.load(std::memory_order_relaxed) == self)
        {
            ++m_depth;
            return true;
        }
        if (!m_mutex.try_lock())
            return false;
        m_owner.store(self, std::memory_order_relaxed);
        m_depth = 1u;
        return true;
    }

    void unlock()
    {
        if (--m_depth == 0u)
        {
            m_owner.store(std::thread::id{}, std::memory_order_relaxed);
            m_mutex.unlock();
        }
    }

private:
    std::mutex m_mutex;
    std::atomic<std::thread::id> m_owner{};
    uint32_t m_depth = 0u; // touched only by the owner
};

namespace GSSharedPresent { struct Frame; }

struct GSDebugSnapshot
{
    GSContext ctx[2]{};
    GSPrimReg prim{};
    GSTexaReg texa{};
    GSTexClutReg texclut{};
    uint64_t scanmsk = 0;
    uint64_t dimx = 0;
    uint64_t dthe = 0;
    uint64_t colclamp = 0;
    GSBitBltBuf bitbltbuf{};
    GSTrxPos trxpos{};
    GSTrxReg trxreg{};
    uint32_t trxdir = 0;
    uint32_t transferX = 0;
    uint32_t transferY = 0;
    uint32_t transferTotalPixels = 0;
    uint32_t transferCopiedPixels = 0;
    uint32_t lastDisplayBaseBytes = 0;
    GSFrameReg preferredDisplaySourceFrame{};
    uint32_t preferredDisplayDestFbp = 0;
    bool hasPreferredDisplaySource = false;
    uint32_t hostPresentationWidth = 0;
    uint32_t hostPresentationHeight = 0;
    uint32_t hostPresentationDisplayFbp = 0;
    uint32_t hostPresentationSourceFbp = 0;
    bool hostPresentationUsedPreferred = false;
    bool hasHostPresentationFrame = false;
    size_t localToHostPendingBytes = 0;
};

enum class GSDebugEventKind : uint8_t
{
    GifTag = 0,
    Register = 1,
    Draw = 2,
    Transfer = 3,
    Present = 4,
};

struct GSDebugHistoryEntry
{
    uint64_t seq = 0;
    uint64_t vsyncTick = 0;
    uint32_t frameIndex = 0;
    GSDebugEventKind kind = GSDebugEventKind::Register;

    uint8_t reg = 0;
    uint64_t regValue = 0;

    uint32_t gifSizeBytes = 0;
    uint32_t gifNloop = 0;
    uint8_t gifFlg = 0;
    uint8_t gifNreg = 0;

    GSPrimReg prim{};
    GSFrameReg frame{};
    GSZbufReg zbuf{};
    GSTex0Reg tex0{};
    GSScissorReg scissor{};
    uint64_t test = 0;
    uint64_t alpha = 0;

    uint32_t vertexCount = 0;
    float xMin = 0.0f;
    float xMax = 0.0f;
    float yMin = 0.0f;
    float yMax = 0.0f;
    double zMin = 0.0;
    double zMax = 0.0;
    uint8_t aMin = 0;
    uint8_t aMax = 0;

    GSBitBltBuf bitbltbuf{};
    GSTrxPos trxpos{};
    GSTrxReg trxreg{};
    uint32_t trxdir = 0;
    uint32_t transferPixels = 0;

    uint32_t displayFbp = 0;
    uint32_t sourceFbp = 0;
    uint32_t width = 0;
    uint32_t height = 0;
    bool usedPreferred = false;
};

class GS
{
public:
    GS();
    ~GS() = default;

    void init(uint8_t *vram, uint32_t vramSize, struct GSRegisters *privRegs = nullptr);
    void reset();
    void setRasterBackend(std::unique_ptr<GSRasterBackend> backend);

    void processGIFPacket(const uint8_t *data, uint32_t sizeBytes, GifPathId path = GifPathId::Path1);
    bool processNativePackedGIFPacket(const uint8_t *data, uint32_t sizeBytes, GifPathId path = GifPathId::Path1);
    bool hasPendingGIFPacket(GifPathId path) const; // GOW-Port: the DMA shortcuts cannot skip a pending stream.
    void uploadImageNative(uint64_t bitbltbuf,
                           uint64_t trxpos,
                           uint64_t trxreg,
                           uint64_t trxdir,
                           const uint8_t *data,
                           uint32_t sizeBytes,
                           uint64_t setupGifTag = 0u); // GOW-Port: effects of the setup validated by DMA.
    void writeRegister(uint8_t regAddr, uint64_t value);

    const uint8_t *lockDisplaySnapshot(uint32_t &outSize);
    void unlockDisplaySnapshot();
    uint32_t getLastDisplayBaseBytes() const;
    const GSFrameReg &getContextFrame(int index) const
    {
        return m_ctx[(index != 0) ? 1 : 0].frame;
    }
    GSDebugSnapshot getDebugSnapshot() const;
    std::vector<GSDebugHistoryEntry> getDebugHistory() const;
    void clearDebugHistory();
    bool isDebugHistoryPaused() const;
    void setDebugHistoryPaused(bool paused);
    bool getPreferredDisplaySource(GSFrameReg &outSource, uint32_t &outDestFbp) const;
    void latchHostPresentationFrame();
    // GOW-Port: the host registers the acquired texture, never the later request.
    bool usesSharedPresentation() const;
    void recordAcquiredSharedFrame(const GSSharedPresent::Frame &frame);
    bool copyLatchedHostPresentationFrame(std::vector<uint8_t> &outPixels,
                                          uint32_t &outWidth,
                                          uint32_t &outHeight,
                                          uint32_t *outDisplayFbp = nullptr,
                                          uint32_t *outSourceFbp = nullptr,
                                          bool *outUsedPreferred = nullptr) const;
    bool clearFramebufferContext(uint32_t contextIndex, uint32_t rgba);
    bool clearActiveFramebuffer(uint32_t rgba);
    uint64_t nativeImageUploadCount() const { return m_nativeImageUploadCount; }
    uint64_t nativePackedGIFPacketCount() const { return m_nativePackedGIFPacketCount; }

    uint32_t consumeLocalToHostBytes(uint8_t *dst, uint32_t maxBytes);

    void refreshDisplaySnapshot();

    void WriteVram(uint32_t psm, uint32_t base, uint32_t bw, uint32_t x, uint32_t y, uint32_t value);
    uint32_t ReadVram(uint32_t psm, uint32_t base, uint32_t bw, uint32_t x, uint32_t y) const;

private:
    void snapshotVRAM();
    void writeRegisterUnlocked(uint8_t regAddr, uint64_t value);
    void writeRegisterPacked(uint8_t regDesc, uint64_t lo, uint64_t hi);
    void uploadImageNativeUnlocked(uint64_t bitbltbuf,
                                   uint64_t trxpos,
                                   uint64_t trxreg,
                                   uint64_t trxdir,
                                   const uint8_t *data,
                                   uint32_t sizeBytes);
    void vertexKick(bool drawing);

    void recordDebugEventUnlocked(GSDebugHistoryEntry entry, uint64_t tickOverride = UINT64_MAX);
    GSDebugHistoryEntry makeDebugEventUnlocked(GSDebugEventKind kind) const;
    void recordGifTagDebugEventUnlocked(uint32_t sizeBytes, uint32_t nloop, uint8_t flg, uint32_t nreg);
    void recordRegisterDebugEventUnlocked(uint8_t regAddr, uint64_t value);
    void recordDrawDebugEventUnlocked(int vertexCount);
    void recordTransferDebugEventUnlocked();
    void recordPresentDebugEventUnlocked(uint32_t displayFbp, uint32_t sourceFbp, uint32_t width, uint32_t height, bool usedPreferred, uint64_t tickOverride = UINT64_MAX);

    void processImageData(const uint8_t *data, uint32_t sizeBytes);
    bool tryProcessNativeImageUploadPacket(const uint8_t *data, uint32_t sizeBytes);
    GSPrimitiveBatch buildDrawBatch(int vertexCount) const;
    void updatePreferredDisplaySourceForDraw(const GSPrimitiveBatch &batch);
    GSPresentationRequest buildPresentationRequestUnlocked() const;


    GSContext &activeContext();

    uint8_t *m_localMemoryStorage = nullptr;
    uint32_t m_localMemorySize = 0u;
    struct GSRegisters *m_privRegs = nullptr;
    mutable GSStateMutex m_stateMutex; // GOW-Port: was std::recursive_mutex (see GSStateMutex)
    mutable std::mutex m_backendLifetimeMutex;
    mutable std::mutex m_presentationMutex;

    // GOW-Port: cursor bounded per PATH, without storing whole packets or payloads.
    struct GifInputState
    {
        std::array<uint8_t, 16> partial{};
        std::array<uint8_t, 16> regs{};
        uint32_t partialBytes = 0u, registersLeft = 0u, imageBytes = 0u, paddingBytes = 0u;
        uint32_t nreg = 0u, regIndex = 0u;
        uint8_t format = 0u;
        bool pending() const { return partialBytes || registersLeft || imageBytes || paddingBytes; }
    };
    std::array<GifInputState, 4> m_gifInput{};

    GSContext m_ctx[2];
    GSPrimReg m_prim{};
    GSPrimReg m_primRegister{};
    GSPrimReg m_prmodeRegister{};

    uint8_t m_curR = 0x80, m_curG = 0x80, m_curB = 0x80, m_curA = 0x80;
    float m_curQ = 1.0f;
    float m_packedStQ = 1.0f; // Q of the last PACKED ST write: becomes RGBAQ.Q only at the next PACKED RGBAQ write (GS rule)
    float m_curS = 0.0f, m_curT = 0.0f;
    uint16_t m_curU = 0, m_curV = 0;
    uint8_t m_curFog = 0;
    uint8_t m_fogR = 0, m_fogG = 0, m_fogB = 0;

    bool m_prmodecont = true;
    // GOW-Port: early FINISH (ps2_memory.h): PATH2 FINISH writes of the current worker job the game already received, and the
    // path of the packet being processed. Stage thread only.
    uint32_t m_finishGiven = 0;
    GifPathId m_currentPath = GifPathId::Path1;
    void stageMarker(const uint8_t *data, uint32_t sizeBytes);
    bool m_pabe = false;
    uint64_t m_scanmsk = 0;
    uint64_t m_dimx = 0;
    uint64_t m_dthe = 0;
    uint64_t m_colclamp = 0;
    GSTexaReg m_texa{0u, false, 0u};
    GSTexClutReg m_texclut{0u, 0u, 0u};

    GSBitBltBuf m_bitbltbuf{};
    GSTrxPos m_trxpos{};
    GSTrxReg m_trxreg{};
    uint32_t m_trxdir = 3;


    static constexpr int kMaxVerts = 6;
    GSVertex m_vtxQueue[kMaxVerts];
    int m_vtxCount = 0;
    int m_vtxIndex = 0;

    std::vector<uint8_t> m_displaySnapshot;
    std::mutex m_snapshotMutex;
    uint32_t m_lastDisplayBaseBytes = 0;
    GSFrameReg m_preferredDisplaySourceFrame{};
    uint32_t m_preferredDisplayDestFbp = 0;
    bool m_hasPreferredDisplaySource = false;
    std::vector<uint8_t> m_hostPresentationFrame;
    uint32_t m_hostPresentationWidth = 0;
    uint32_t m_hostPresentationHeight = 0;
    uint32_t m_hostPresentationDisplayFbp = 0;
    uint32_t m_hostPresentationSourceFbp = 0;
    bool m_hostPresentationUsedPreferred = false;
    bool m_hasHostPresentationFrame = false;
    // GOW-Port: the explicit capture keeps the CRTC of the last latch; GL stays on the worker.
    GSPresentationRequest m_hostPresentationRequest{};
    bool m_hasHostPresentationRequest = false;
    bool m_hostPresentationRequestIsShared = false;
    uint64_t m_hostPresentationGeneration = 0u;
    uint64_t m_hostSharedSequence = 0u;
    mutable PresentationFrame m_hostDiagnosticFrame;
    mutable uint64_t m_hostDiagnosticGeneration = UINT64_MAX;
    uint64_t m_nativeImageUploadCount = 0;
    uint64_t m_nativePackedGIFPacketCount = 0;

    static constexpr size_t kDebugHistoryCapacity = 512;
    std::array<GSDebugHistoryEntry, kDebugHistoryCapacity> m_debugHistory{};
    size_t m_debugHistoryWrite = 0;
    size_t m_debugHistoryCount = 0;
    uint64_t m_debugNextSeq = 1;
    uint32_t m_debugFrameIndex = 0;
    uint64_t m_debugLastVsyncTick = UINT64_MAX;
    bool m_debugHistoryPaused = true;

    std::unique_ptr<GSRasterBackend> m_backend;
};

#endif
