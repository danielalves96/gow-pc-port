#include <map>
#include <chrono>
#include <string>
// Based on Blackline Interactive implementation
#include "runtime/ps2_memory.h"
#include <cstring>
#include <vector>
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <immintrin.h>

enum VIFCmd : uint8_t
{
    VIF_NOP = 0x00,
    VIF_STCYCL = 0x01,
    VIF_OFFSET = 0x02,
    VIF_BASE = 0x03,
    VIF_ITOP = 0x04,
    VIF_STMOD = 0x05,
    VIF_MSKPATH3 = 0x06,
    VIF_MARK = 0x07,
    VIF_FLUSHE = 0x10,
    VIF_FLUSH = 0x11,
    VIF_FLUSHA = 0x13,
    VIF_MSCAL = 0x14,
    VIF_MSCALF = 0x15,
    VIF_MSCNT = 0x17,
    VIF_STMASK = 0x20,
    VIF_STROW = 0x30,
    VIF_STCOL = 0x31,
    VIF_MPG = 0x4A,
    VIF_DIRECT = 0x50,
    VIF_DIRECTHL = 0x51,
};

void PS2Memory::processVIF0Data(uint32_t srcPhys, uint32_t sizeBytes)
{
    if (sizeBytes == 0u || srcPhys >= PS2_RAM_SIZE)
        return;

    const uint64_t requestedEnd = static_cast<uint64_t>(srcPhys) + static_cast<uint64_t>(sizeBytes);
    if (requestedEnd > static_cast<uint64_t>(PS2_RAM_SIZE))
        sizeBytes = PS2_RAM_SIZE - srcPhys;

    processVIF0Data(m_rdram + srcPhys, sizeBytes);
}

void PS2Memory::processVIF0Data(const uint8_t *data, uint32_t sizeBytes)
{
    if (sizeBytes == 0u)
        return;

    uint32_t pos = 0;
    while (pos + 4 <= sizeBytes)
    {
        uint32_t cmd = 0u;
        std::memcpy(&cmd, data + pos, sizeof(cmd));
        pos += 4u;

        const uint8_t opcode = static_cast<uint8_t>((cmd >> 24) & 0x7Fu);
        const uint16_t imm = static_cast<uint16_t>(cmd & 0xFFFFu);
        const uint8_t num = static_cast<uint8_t>((cmd >> 16) & 0xFFu);
        const bool irq = (cmd & 0x80000000u) != 0u;

        vif0_regs.code = cmd;
        vif0_regs.num = num;
        if (irq)
            vif0_regs.stat |= (1u << 11);

        if (opcode == VIF_NOP)
        {
            continue;
        }
        else if (opcode == VIF_STCYCL)
        {
            vif0_regs.cycle = imm;
            continue;
        }
        else if (opcode == VIF_ITOP)
        {
            vif0_regs.itops = imm & 0x3FFu;
            continue;
        }
        else if (opcode == VIF_STMOD)
        {
            vif0_regs.mode = imm & 3u;
            continue;
        }
        else if (opcode == VIF_MARK)
        {
            vif0_regs.mark = imm;
            vif0_regs.stat |= (1u << 6);
            continue;
        }
        else if (opcode == VIF_FLUSHE || opcode == VIF_FLUSH || opcode == VIF_FLUSHA)
        {
            continue;
        }
        else if (opcode == VIF_STMASK)
        {
            if (pos + 4u > sizeBytes)
                break;
            std::memcpy(&vif0_regs.mask, data + pos, sizeof(vif0_regs.mask));
            pos += 4u;
            continue;
        }
        else if (opcode == VIF_STROW)
        {
            if (pos + 16u > sizeBytes)
                break;
            std::memcpy(vif0_regs.row, data + pos, 16u);
            pos += 16u;
            continue;
        }
        else if (opcode == VIF_STCOL)
        {
            if (pos + 16u > sizeBytes)
                break;
            std::memcpy(vif0_regs.col, data + pos, 16u);
            pos += 16u;
            continue;
        }
        else if (opcode == VIF_MPG)
        {
            const uint32_t destAddr = static_cast<uint32_t>(imm & 0x1FFu) * 8u;
            const uint32_t instructionCount = (num == 0u) ? 256u : static_cast<uint32_t>(num);
            const uint32_t mpgBytes = instructionCount * 8u;
            uint32_t copyBytes = 0u;
            if (m_vu0Code && destAddr < PS2_VU0_CODE_SIZE && mpgBytes > 0u)
            {
                copyBytes = mpgBytes;
                if (destAddr + copyBytes > PS2_VU0_CODE_SIZE)
                    copyBytes = PS2_VU0_CODE_SIZE - destAddr;
                if (pos + copyBytes <= sizeBytes)
                {
                    std::memcpy(m_vu0Code + destAddr, data + pos, copyBytes);
                    markVU0CodeModified();
                }
            }

            pos += mpgBytes;
            if (pos > sizeBytes)
                break;
            continue;
        }
        else if ((opcode & 0x60u) == 0x60u)
        {
            const uint8_t vn = static_cast<uint8_t>((opcode >> 2) & 0x3u);
            const uint8_t vl = static_cast<uint8_t>(opcode & 0x3u);
            const int components = static_cast<int>(vn) + 1;
            int bitsPerComponent = 32;
            switch (vl)
            {
            case 0:
                bitsPerComponent = 32;
                break;
            case 1:
                bitsPerComponent = 16;
                break;
            case 2:
                bitsPerComponent = 8;
                break;
            case 3:
                bitsPerComponent = (vn == 3u) ? 4 : 16;
                break;
            default:
                break;
            }
            const int bitsPerVector = (vl == 3u && vn == 3u) ? 16 : (components * bitsPerComponent);
            uint32_t bytesPerVector = static_cast<uint32_t>((bitsPerVector + 7) / 8);
            const uint32_t writeVectorCount = (num == 0u) ? 256u : static_cast<uint32_t>(num);
            uint32_t cl = vif0_regs.cycle & 0xFFu;
            uint32_t wl = (vif0_regs.cycle >> 8) & 0xFFu;
            if (cl == 0u)
                cl = 1u;
            if (wl == 0u)
                wl = 1u;
            uint32_t sourceVectorCount = writeVectorCount;
            if (cl < wl)
            {
                const uint32_t fullBlocks = writeVectorCount / wl;
                uint32_t remainder = writeVectorCount % wl;
                if (remainder > cl)
                    remainder = cl;
                sourceVectorCount = fullBlocks * cl + remainder;
            }
            uint32_t totalBytes = sourceVectorCount * bytesPerVector;
            totalBytes = (totalBytes + 3u) & ~3u;

            if (m_vu0Data && pos + totalBytes <= sizeBytes && vl == 0u)
            {
                uint32_t vuAddr = static_cast<uint32_t>(imm & 0x3FFu);
                if ((imm & 0x8000u) != 0u)
                    vuAddr = (vuAddr + (vif0_regs.tops & 0x3FFu)) & 0x3FFu;
                const uint8_t *srcBase = data + pos;
                uint32_t srcIndex = 0u;
                for (uint32_t writeIndex = 0; writeIndex < writeVectorCount; ++writeIndex)
                {
                    const uint32_t cyclePos = writeIndex % wl;
                    const bool sourceAvailable = (cl >= wl) || (cyclePos < cl);
                    uint32_t destVec = (cl >= wl) ? ((vuAddr + (writeIndex / wl) * cl + cyclePos) & 0x3FFu)
                                                  : ((vuAddr + writeIndex) & 0x3FFu);
                    const uint32_t destOff = destVec * 16u;
                    if (destOff + 16u > PS2_VU0_DATA_SIZE)
                    {
                        if (sourceAvailable && srcIndex < sourceVectorCount)
                            ++srcIndex;
                        continue;
                    }
                    if (!sourceAvailable || srcIndex >= sourceVectorCount)
                        continue;
                    const uint8_t *srcVec = srcBase + srcIndex * bytesPerVector;
                    ++srcIndex;
                    uint32_t lanes[4] = {0u, 0u, 0u, 0u};
                    std::memcpy(lanes, m_vu0Data + destOff, sizeof(lanes));
                    const uint32_t limit = (components > 4) ? 4u : static_cast<uint32_t>(components);
                    for (uint32_t c = 0; c < limit; ++c)
                    {
                        uint32_t scalar = 0u;
                        std::memcpy(&scalar, srcVec + c * 4u, sizeof(scalar));
                        lanes[c] = scalar;
                    }
                    _mm_storeu_si128(reinterpret_cast<__m128i *>(m_vu0Data + destOff), _mm_loadu_si128(reinterpret_cast<const __m128i *>(lanes)));
                }
            }
            pos += totalBytes;
            if (pos > sizeBytes)
                break;
            continue;
        }
        else
        {
            break;
        }
    }
}

namespace
{
// GOW-Port: UNPACK without mask and with CL >= WL, specialized by format (VL, components) and STMOD. It writes the same as the
// general loop of processVIF1Data (the components the format does not bring keep their previous value; V2 repeats XY).
// VL 3 is V4-5 (VN 3), which ignores STMOD.
template <uint32_t VL, uint32_t COMP, uint32_t MODE>
void unpackUnmasked(uint8_t *vuData, const uint8_t *src, uint32_t bytesPerVector, uint32_t count, uint32_t vuAddr,
                      uint32_t cl, uint32_t wl, bool zeroExtend, uint32_t *row)
{
    uint32_t block = 0u, cyclePos = 0u;
    for (uint32_t i = 0; i < count; ++i, src += bytesPerVector)
    {
        const uint32_t destVec = (vuAddr + block * cl + cyclePos) & 0x3FFu;
        if (++cyclePos == wl)
        {
            cyclePos = 0u;
            ++block;
        }
        uint8_t *dst = vuData + destVec * 16u;
        // GOW-Port: without STMOD, V4-16 / V4-8 / V2-16 / V2-8 (the formats that weigh most in combat) are a conversion of
        // signed or zero-extended integers: one SSE4.1 instruction instead of four scalar conversions. Same result bit for bit.
        if constexpr (MODE == 0u && (VL == 1u || VL == 2u) && (COMP == 4u || COMP == 2u))
        {
            __m128i v;
            if constexpr (VL == 1u)
            {
                if constexpr (COMP == 4u)
                    v = _mm_loadl_epi64(reinterpret_cast<const __m128i *>(src));
                else
                {
                    uint32_t raw;
                    std::memcpy(&raw, src, 4);
                    v = _mm_cvtsi32_si128(static_cast<int>(raw));
                }
                v = zeroExtend ? _mm_cvtepu16_epi32(v) : _mm_cvtepi16_epi32(v);
            }
            else
            {
                uint32_t raw;
                if constexpr (COMP == 4u)
                    std::memcpy(&raw, src, 4);
                else
                {
                    uint16_t r16;
                    std::memcpy(&r16, src, 2);
                    raw = r16;
                }
                v = _mm_cvtsi32_si128(static_cast<int>(raw));
                v = zeroExtend ? _mm_cvtepu8_epi32(v) : _mm_cvtepi8_epi32(v);
            }
            if constexpr (COMP == 2u)
                v = _mm_shuffle_epi32(v, _MM_SHUFFLE(1, 0, 1, 0)); // V2 repite XY en ZW
            _mm_storeu_si128(reinterpret_cast<__m128i *>(dst), v);
            continue;
        }
        uint32_t d[4];
        std::memcpy(d, dst, sizeof(d));

        if constexpr (VL == 0u)
        {
            if constexpr (COMP == 1u)
            {
                uint32_t scalar;
                std::memcpy(&scalar, src, 4);
                d[0] = d[1] = d[2] = d[3] = scalar;
            }
            else
            {
                std::memcpy(d, src, COMP * 4u); // memory is still little-endian, the same as the per-component read
            }
        }
        else if constexpr (VL == 1u)
        {
            const auto ext = [&](uint16_t raw) -> uint32_t
            { return zeroExtend ? static_cast<uint32_t>(raw) : static_cast<uint32_t>(static_cast<int32_t>(static_cast<int16_t>(raw))); };
            if constexpr (COMP == 1u)
            {
                uint16_t raw;
                std::memcpy(&raw, src, 2);
                d[0] = d[1] = d[2] = d[3] = ext(raw);
            }
            else
            {
                for (uint32_t c = 0; c < COMP; ++c)
                {
                    uint16_t raw;
                    std::memcpy(&raw, src + c * 2u, 2);
                    d[c] = ext(raw);
                }
            }
        }
        else if constexpr (VL == 2u)
        {
            const auto ext = [&](uint8_t raw) -> uint32_t
            { return zeroExtend ? static_cast<uint32_t>(raw) : static_cast<uint32_t>(static_cast<int32_t>(static_cast<int8_t>(raw))); };
            if constexpr (COMP == 1u)
                d[0] = d[1] = d[2] = d[3] = ext(src[0]);
            else
                for (uint32_t c = 0; c < COMP; ++c)
                    d[c] = ext(src[c]);
        }
        else
        {
            uint16_t packed;
            std::memcpy(&packed, src, 2);
            d[0] = (packed & 0x001Fu) << 3u;
            d[1] = (packed & 0x03E0u) >> 2u;
            d[2] = (packed & 0x7C00u) >> 7u;
            d[3] = (packed & 0x8000u) >> 8u;
        }

        if constexpr (COMP == 2u)
        {
            d[2] = d[0];
            d[3] = d[1];
        }

        if constexpr (VL != 3u && MODE != 0u)
        {
            for (uint32_t f = 0; f < 4u; ++f)
            {
                if constexpr (MODE == 3u)
                {
                    row[f] = d[f];
                }
                else
                {
                    d[f] += row[f];
                    if constexpr (MODE == 2u)
                        row[f] = d[f];
                }
            }
        }
        std::memcpy(dst, d, sizeof(d));
    }
}

template <uint32_t VL, uint32_t COMP>
void unpackUnmaskedMode(uint32_t mode, uint8_t *vuData, const uint8_t *src, uint32_t bytesPerVector, uint32_t count,
                          uint32_t vuAddr, uint32_t cl, uint32_t wl, bool zeroExtend, uint32_t *row)
{
    switch (mode)
    {
    case 1u:
        unpackUnmasked<VL, COMP, 1u>(vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);
        break;
    case 2u:
        unpackUnmasked<VL, COMP, 2u>(vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);
        break;
    case 3u:
        unpackUnmasked<VL, COMP, 3u>(vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);
        break;
    default:
        unpackUnmasked<VL, COMP, 0u>(vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);
        break;
    }
}

// false if the format does not cover it (VL 3 except V4-5): the general loop handles it.
bool unpackUnmaskedDispatch(uint32_t vl, uint32_t vn, uint32_t mode, uint8_t *vuData, const uint8_t *src,
                               uint32_t bytesPerVector, uint32_t count, uint32_t vuAddr, uint32_t cl, uint32_t wl,
                               bool zeroExtend, uint32_t *row)
{
#define GOW_UNPACK_VN(VLV)                                                                                                   \
    switch (vn)                                                                                                              \
    {                                                                                                                        \
    case 0u:                                                                                                                 \
        unpackUnmaskedMode<VLV, 1u>(mode, vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);           \
        return true;                                                                                                         \
    case 1u:                                                                                                                 \
        unpackUnmaskedMode<VLV, 2u>(mode, vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);           \
        return true;                                                                                                         \
    case 2u:                                                                                                                 \
        unpackUnmaskedMode<VLV, 3u>(mode, vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);           \
        return true;                                                                                                         \
    default:                                                                                                                 \
        unpackUnmaskedMode<VLV, 4u>(mode, vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);           \
        return true;                                                                                                         \
    }
    switch (vl)
    {
    case 0u:
        GOW_UNPACK_VN(0u)
    case 1u:
        GOW_UNPACK_VN(1u)
    case 2u:
        GOW_UNPACK_VN(2u)
    default:
        if (vn == 3u)
        {
            unpackUnmasked<3u, 4u, 0u>(vuData, src, bytesPerVector, count, vuAddr, cl, wl, zeroExtend, row);
            return true;
        }
        return false;
    }
#undef GOW_UNPACK_VN
}
} // namespace

void PS2Memory::processVIF1Data(uint32_t srcPhys, uint32_t sizeBytes)
{
    if (sizeBytes == 0u || srcPhys >= PS2_RAM_SIZE)
        return;

    const uint64_t requestedEnd = static_cast<uint64_t>(srcPhys) + static_cast<uint64_t>(sizeBytes);
    if (requestedEnd > static_cast<uint64_t>(PS2_RAM_SIZE))
        sizeBytes = PS2_RAM_SIZE - srcPhys;

    processVIF1Data(m_rdram + srcPhys, sizeBytes);
}

// GOW-Port: send the original bytes; the per-PATH GIF cursor keeps PACKED/REGLIST/IMAGE.
// Do not prepend another IMAGE tag to a payload that already continues an open tag.
void PS2Memory::submitVif1DirectPayload(const uint8_t *data, uint32_t sizeBytes, bool directHl)
{
    submitGifPacket(GifPathId::Path2, data, sizeBytes, true, directHl);
}

void PS2Memory::processVIF1Data(const uint8_t *data, uint32_t sizeBytes)
{
    syncGsWorkerIfOtherThread("processVIF1Data"); // GOW-Port: VIF1 FIFO writes etc. run behind queued worker jobs
    if (sizeBytes == 0u)
        return;

    // GOW-Port: GOW_VIF_CAPTURE="<folder>:<second>": the first VIF1 send of more than 1 MB from that second is saved with
    // the VU1 memories, the VRAM and the VIF1 registers (format of tools/render/replay_vif_chain.cpp, the VIF1/VU1
    // test bench). They are game data: local only, never published.
    {
        static const char *spec = std::getenv("GOW_VIF_CAPTURE");
        static bool dumped = false;
        if (spec && !dumped && sizeBytes > 1000000u)
        {
            static const auto t0 = std::chrono::steady_clock::now();
            std::string folder = spec;
            double afterSeconds = 0.0;
            if (const size_t colon = folder.rfind(':'); colon != std::string::npos && colon > 1u)
            {
                afterSeconds = std::atof(folder.c_str() + colon + 1);
                folder.resize(colon);
            }
            if (std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count() >= afterSeconds)
            {
                dumped = true;
                const auto save = [&](const char *name, const void *buffer, size_t bytes)
                {
                    if (std::FILE *f = std::fopen((folder + "/" + name).c_str(), "wb"))
                    {
                        std::fwrite(buffer, 1, bytes, f);
                        std::fclose(f);
                    }
                };
                save("vif1_stream.bin", data, sizeBytes);
                save("vu1MicroMem.bin", getVU1Code(), PS2_VU1_CODE_SIZE);
                save("vu1Memory.bin", getVU1Data(), PS2_VU1_DATA_SIZE);
                save("vram.bin", getGSVRAM(), PS2_GS_VRAM_SIZE);
                std::vector<uint8_t> hw(0x4000u + 0x1000u, 0u); // VIF1 registers at the positions the tool reads
                const auto put = [&](uint32_t regOffset, uint32_t value) { std::memcpy(hw.data() + regOffset, &value, 4); };
                put(0x3C00, vif1_regs.stat); put(0x3C40, vif1_regs.cycle); put(0x3C50, vif1_regs.mode); put(0x3C70, vif1_regs.mask);
                put(0x3C90, vif1_regs.itops); put(0x3CA0, vif1_regs.base); put(0x3CB0, vif1_regs.ofst); put(0x3CC0, vif1_regs.tops);
                put(0x3CD0, vif1_regs.itop); put(0x3CE0, vif1_regs.top);
                for (uint32_t i = 0; i < 4u; ++i)
                {
                    put(0x3D00 + i * 16u, vif1_regs.row[i]);
                    put(0x3D40 + i * 16u, vif1_regs.col[i]);
                }
                save("eeHwRegs.bin", hw.data(), hw.size());
                std::fprintf(stderr, "[vif1] capture of %u bytes saved to %s\n", sizeBytes, folder.c_str());
            }
        }
    }

    uint32_t pos = 0;
    static const bool traceVif = std::getenv("GOW_VIF_DIAG") != nullptr;
    // GOW-Port: the last commands of this DMA, to diagnose lost alignment. Always kept (one store per command), so the
    // first stream fault of a player's session prints them without GOW_VIF_DIAG (issue #34: faults right after a save).
    struct VifCommand { uint32_t offset; uint32_t code; };
    VifCommand commands[8] = {};
    uint32_t commandCount = 0u;

    while (pos < sizeBytes)
    {
        // GOW-Port: the command size persists between DMA blocks and FIFO writes.
        // Only truncated payloads are accumulated; a complete DIRECT does not need this copy.
        if (m_vif1DirectBytesRemaining != 0u)
        {
            const uint32_t chunk = std::min<uint32_t>(m_vif1DirectBytesRemaining, sizeBytes - pos);
            m_vif1DirectPayload.insert(m_vif1DirectPayload.end(), data + pos, data + pos + chunk);
            pos += chunk;
            m_vif1DirectBytesRemaining -= chunk;
            if (m_vif1DirectBytesRemaining != 0u)
                break;
            if (traceVif)
            {
                static unsigned completed = 0u;
                if (++completed <= 16u)
                    std::fprintf(stderr, "[gow-vif-direct] completed=%u bytes=%u hl=%u\n", completed,
                                 static_cast<uint32_t>(m_vif1DirectPayload.size()), m_vif1DirectHl ? 1u : 0u);
            }
            submitVif1DirectPayload(m_vif1DirectPayload.data(), static_cast<uint32_t>(m_vif1DirectPayload.size()), m_vif1DirectHl);
            m_vif1DirectPayload.clear();
            m_vif1DirectHl = false;
            continue;
        }
        if (sizeBytes - pos < 4u)
            break;
        // GOW-Port: adapted from Claude, god-of-war-recomp 540defd. A pending IMAGE of PATH2
        // only continues with the payload of the next DIRECT/DIRECTHL.
        // Before, the VIF codes following the DIRECT were taken as pixels and the stream lost sync.

        uint32_t cmd;
        const uint32_t cmdOffset = pos;
        memcpy(&cmd, data + pos, 4);
        pos += 4;
        commands[commandCount++ & 7u] = {cmdOffset, cmd};

        uint8_t opcode = (cmd >> 24) & 0x7F;
        uint16_t imm = cmd & 0xFFFF;
        uint8_t num = (cmd >> 16) & 0xFF;
        const bool irq = (cmd & 0x80000000u) != 0u;

        // Track most-recent command for VIFn_CODE emulation.
        if (traceVif)
        {
            static uint64_t commands = 0u, irqCommands = 0u;
            ++commands;
            if (irq) ++irqCommands;
            if (commands == 1u || (commands % 4096u) == 0u || (irq && irqCommands <= 32u))
                std::fprintf(stderr, "[gow-vif] commands=%llu irq=%llu code=0x%08X offset=0x%X bytes=%u tops=%u\n",
                             static_cast<unsigned long long>(commands), static_cast<unsigned long long>(irqCommands),
                             cmd, pos - 4u, sizeBytes, vif1_regs.tops);
        }
        vif1_regs.code = cmd;
        vif1_regs.num = num;
        if (irq)
            vif1_regs.stat |= (1u << 11); // INT

        if (opcode == VIF_NOP)
        {
            continue;
        }
        else if (opcode == VIF_STCYCL)
        {
            vif1_regs.cycle = imm;
            static const bool cycleDiag = std::getenv("GOW_VIF_CYCLE_DIAG") != nullptr;
            if (cycleDiag)
            {
                static std::map<uint32_t, unsigned> cycles;
                static unsigned total = 0;
                ++cycles[imm & 0xFFFFu];
                if (++total % 50000u == 0u)
                    for (auto &kv : cycles)
                        std::fprintf(stderr, "[vifcycle] cl=%u wl=%u x%u\n", kv.first & 0xFFu, (kv.first >> 8) & 0xFFu, kv.second);
            }
            continue;
        }
        else if (opcode == VIF_OFFSET)
        {
            // VIF double-buffer setup. OFFSET clears DBF and resets TOPS to BASE.
            // Do not rewrite BASE from the previous TOPS value.
            vif1_regs.ofst = imm & 0x3FFu;
            vif1_regs.tops = vif1_regs.base & 0x3FFu;
            vif1_regs.stat &= ~(1u << 7); // clear DBF
            continue;
        }
        else if (opcode == VIF_BASE)
        {
            // BASE only updates the base register. TOPS changes on OFFSET/MSCAL.
            vif1_regs.base = imm & 0x3FFu;
            continue;
        }
        else if (opcode == VIF_ITOP)
        {
            // ITOP VIFcode writes pending ITOPS; VU XITOP observes it after MSCAL/MSCNT.
            vif1_regs.itops = imm & 0x3FFu;
            continue;
        }
        else if (opcode == VIF_STMOD)
        {
            vif1_regs.mode = imm & 3u;
            continue;
        }
        else if (opcode == VIF_MSKPATH3)
        {
            // VIF command docs: MSKPATH3 uses IMMEDIATE bit 15.
            const bool wasMasked = m_path3Masked;
            m_path3Masked = (imm & 0x8000u) != 0u;
            if (wasMasked && !m_path3Masked)
                flushMaskedPath3Packets();
            continue;
        }
        else if (opcode == VIF_MARK)
        {
            vif1_regs.mark = imm;
            vif1_regs.stat |= (1u << 6); // MRK
            continue;
        }
        else if (opcode == VIF_FLUSHE || opcode == VIF_FLUSH || opcode == VIF_FLUSHA)
        {
            continue;
        }
        else if (opcode == VIF_MSCAL || opcode == VIF_MSCALF)
        {
            uint32_t startPC = (uint32_t)imm * 8u;

            const uint32_t runTop = vif1_regs.tops & 0x3FFu;
            const uint32_t runItop = vif1_regs.itops & 0x3FFu;
            if (traceVif)
            {
                static unsigned launches = 0u;
                if (++launches <= 24u && m_vu1Data)
                {
                    uint64_t lo = 0u, hi = 0u;
                    std::memcpy(&lo, m_vu1Data + runItop * 16u, 8u);
                    std::memcpy(&hi, m_vu1Data + runItop * 16u + 8u, 8u);
                    std::fprintf(stderr, "[gow-vif-launch] launch=%u pc=0x%X top=%u itop=%u input=%016llX:%016llX offset=0x%X\n",
                                 launches, startPC, runTop, runItop,
                                 static_cast<unsigned long long>(hi), static_cast<unsigned long long>(lo), pos - 4u);
                }
            }
            vif1_regs.top = runTop;
            vif1_regs.itop = runItop;

            const bool dbf = (vif1_regs.stat & (1u << 7)) != 0u;
            if (dbf)
                vif1_regs.tops = vif1_regs.base & 0x3FFu;
            else
                vif1_regs.tops = (vif1_regs.base + vif1_regs.ofst) & 0x3FFu;
            vif1_regs.stat ^= (1u << 7); // toggle DBF

            if (m_vu1MscalCallback)
                m_vu1MscalCallback(startPC, runTop, runItop);
            continue;
        }
        else if (opcode == VIF_MSCNT)
        {
            const uint32_t runTop = vif1_regs.tops & 0x3FFu;
            const uint32_t runItop = vif1_regs.itops & 0x3FFu;
            vif1_regs.top = runTop;
            vif1_regs.itop = runItop;

            const bool dbf = (vif1_regs.stat & (1u << 7)) != 0u;
            if (dbf)
                vif1_regs.tops = vif1_regs.base & 0x3FFu;
            else
                vif1_regs.tops = (vif1_regs.base + vif1_regs.ofst) & 0x3FFu;
            vif1_regs.stat ^= (1u << 7); // toggle DBF

            if (m_vu1MscntCallback)
                m_vu1MscntCallback(runTop, runItop);
            continue;
        }
        else if (opcode == VIF_STMASK)
        {
            if (pos + 4 > sizeBytes)
            {
                std::fprintf(stderr, "[vif1] STMASK cut\n");
                break;
            }
            uint32_t maskValue = 0;
            std::memcpy(&maskValue, data + pos, sizeof(maskValue));
            vif1_regs.mask = maskValue;
            pos += 4;
            continue;
        }
        else if (opcode == VIF_STROW)
        {
            if (pos + 16 > sizeBytes)
            {
                std::fprintf(stderr, "[vif1] STROW cut\n");
                break;
            }
            std::memcpy(vif1_regs.row, data + pos, 16);
            pos += 16;
            continue;
        }
        else if (opcode == VIF_STCOL)
        {
            if (pos + 16 > sizeBytes)
            {
                std::fprintf(stderr, "[vif1] STCOL cut\n");
                break;
            }
            std::memcpy(vif1_regs.col, data + pos, 16);
            pos += 16;
            continue;
        }
        else if (opcode == VIF_MPG)
        {
            uint32_t destAddr = (uint32_t)imm * 8u;
            const uint32_t instructionCount = (num == 0u) ? 256u : static_cast<uint32_t>(num);
            const uint32_t mpgBytes = instructionCount * 8u;
            if (m_vu1Code && destAddr < PS2_VU1_CODE_SIZE && mpgBytes > 0)
            {
                uint32_t copyBytes = mpgBytes;
                if (destAddr + copyBytes > PS2_VU1_CODE_SIZE)
                    copyBytes = PS2_VU1_CODE_SIZE - destAddr;
                if (pos + copyBytes <= sizeBytes)
                {
                    // GOW-Port: the game resends the same microprograms every frame; if the bytes are already there nothing
                    // changes and rebuilding the decode cache and invalidating the native blocks is avoided.
                    if (std::memcmp(m_vu1Code + destAddr, data + pos, copyBytes) != 0)
                    {
                        std::memcpy(m_vu1Code + destAddr, data + pos, copyBytes);
                        markVU1CodeModified();
                    }
                }
                else
                    std::fprintf(stderr, "[vif1] MPG cut: dest=%x bytes=%u have=%u\n", destAddr, copyBytes, sizeBytes - pos);
            }
            pos += mpgBytes;
            if (pos > sizeBytes)
            {
                std::fprintf(stderr, "[vif1] MPG overruns buffer by %u\n", pos - sizeBytes);
                break;
            }
            continue;
        }
        else if (opcode == VIF_DIRECT || opcode == VIF_DIRECTHL)
        {
            // GOW-Port: IMMEDIATE=0 means 65536 QW, never more than 1 MiB.
            const uint32_t payloadBytes = ((imm == 0u) ? 65536u : static_cast<uint32_t>(imm)) * 16u;
            const uint32_t availableBytes = sizeBytes - pos;
            const bool directHl = (opcode == VIF_DIRECTHL);
            if (payloadBytes <= availableBytes)
            {
                submitVif1DirectPayload(data + pos, payloadBytes, directHl);
                pos += payloadBytes;
            }
            else
            {
                // GOW-Port: optional bounded probe to check for real cuts in the game.
                if (traceVif)
                {
                    static unsigned fragments = 0u;
                    if (++fragments <= 16u)
                        std::fprintf(stderr, "[gow-vif-direct] fragmented=%u code=0x%08X bytes=%u available=%u\n",
                                     fragments, cmd, payloadBytes, availableBytes);
                }
                m_vif1DirectPayload.clear();
                m_vif1DirectPayload.reserve(payloadBytes);
                m_vif1DirectPayload.insert(m_vif1DirectPayload.end(), data + pos, data + sizeBytes);
                m_vif1DirectBytesRemaining = payloadBytes - availableBytes;
                m_vif1DirectHl = directHl;
                pos = sizeBytes;
            }
            continue;
        }
        else if ((opcode & 0x60) == 0x60)
        {
            uint8_t vn = (opcode >> 2) & 0x3;
            uint8_t vl = opcode & 0x3;
            const bool maskEnable = (opcode & 0x10u) != 0u;
            int components = vn + 1;
            int bitsPerComponent = 32;
            switch (vl)
            {
            case 0:
                bitsPerComponent = 32;
                break;
            case 1:
                bitsPerComponent = 16;
                break;
            case 2:
                bitsPerComponent = 8;
                break;
            case 3:
                bitsPerComponent = (vn == 3) ? 4 : 16;
                break;
            default:
                break;
            }
            int bitsPerVector = (vl == 3 && vn == 3) ? 16 : (components * bitsPerComponent);
            uint32_t bytesPerVector = (bitsPerVector + 7) / 8;
            // UNPACK semantics: NUM is 8-bit and NUM==0 means 256 vectors (writes).
            const uint32_t writeVectorCount = (num == 0u) ? 256u : static_cast<uint32_t>(num);

            // STCYCL controls write cycles for UNPACK.
            uint32_t cl = vif1_regs.cycle & 0xFFu;
            uint32_t wl = (vif1_regs.cycle >> 8) & 0xFFu;
            if (cl == 0u)
                cl = 1u;
            if (wl == 0u)
                wl = 1u;

            uint32_t sourceVectorCount = writeVectorCount;
            if (cl < wl)
            {
                const uint32_t fullBlocks = writeVectorCount / wl;
                uint32_t remainder = writeVectorCount % wl;
                if (remainder > cl)
                    remainder = cl;
                sourceVectorCount = fullBlocks * cl + remainder;
            }

            uint32_t totalBytes = sourceVectorCount * bytesPerVector;
            totalBytes = (totalBytes + 3) & ~3u;

            uint32_t vuAddr = (uint32_t)imm & 0x3FFu;
            if ((imm & 0x8000u) != 0u)
                vuAddr = (vuAddr + (vif1_regs.tops & 0x3FFu)) & 0x3FFu;

            const bool zeroExtend = (imm & 0x4000u) != 0u;

            // GOW-Port: report UNPACKs whose data runs past the end of this buffer (they are dropped below).
            if (totalBytes > 0 && pos + totalBytes > sizeBytes)
            {
                static unsigned cuts = 0;
                if (++cuts <= 30u || cuts % 1000u == 0u)
                    std::fprintf(stderr, "[vif1] UNPACK cut #%u: vn=%u vl=%u num=%u addr=%03x need=%u have=%u\n", cuts,
                                 static_cast<unsigned>(vn), static_cast<unsigned>(vl), static_cast<unsigned>(num),
                                 static_cast<unsigned>(imm & 0x3FFu), static_cast<unsigned>(totalBytes),
                                 static_cast<unsigned>(sizeBytes - pos));
            }
            // GOW-Port: GOW_VIF_MATDUMP=<file>[:<start s>[:<len s>]] appends every big V4-32 UNPACK (>= 32 vectors) seen in
            // the window as [u32 vuAddr][u32 vectors][data] to look at the skinning matrix palette Kratos sends to VU1.
            {
                static const char *dumpSpec = std::getenv("GOW_VIF_MATDUMP");
                if (dumpSpec && totalBytes > 0)
                {
                    static std::map<uint32_t, unsigned> hist; static unsigned n = 0;
                    ++hist[(vn << 24) | (vl << 16) | writeVectorCount];
                    if ((++n % 20000u) == 0u)
                        for (auto &kv : hist) if (kv.second > 200u) std::fprintf(stderr, "[vifhist] vn=%u vl=%u num=%u x%u\n", kv.first >> 24, (kv.first >> 16) & 255, kv.first & 0xFFFF, kv.second);
                }
                if (dumpSpec && vn == 3u && vl == 0u && writeVectorCount >= 32u && totalBytes > 0 && pos + totalBytes <= sizeBytes)
                {
                    static const auto t0 = std::chrono::steady_clock::now();
                    static std::string path = dumpSpec;
                    static double startS = 0.0, lenS = 1.0;
                    static bool parsed = [] {
                        const size_t c1 = path.find(':');
                        if (c1 != std::string::npos)
                        {
                            startS = std::atof(path.c_str() + c1 + 1);
                            const size_t c2 = path.find(':', c1 + 1);
                            if (c2 != std::string::npos) lenS = std::atof(path.c_str() + c2 + 1);
                            path.resize(c1);
                        }
                        return true;
                    }();
                    (void)parsed;
                    const double now = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
                    if (now >= startS && now < startS + lenS)
                        if (FILE *f = std::fopen(path.c_str(), "ab"))
                        {
                            const uint32_t hdr[2] = {vuAddr, writeVectorCount};
                            std::fwrite(hdr, 4, 2, f);
                            std::fwrite(data + pos, 1, totalBytes, f);
                            std::fclose(f);
                        }
                }
            }
            bool unpackDone = false;
            if (m_vu1Data && totalBytes > 0 && pos + totalBytes <= sizeBytes && !maskEnable && cl >= wl)
                unpackDone = unpackUnmaskedDispatch(vl, vn, vif1_regs.mode & 3u, m_vu1Data, data + pos, bytesPerVector,
                                                        writeVectorCount, vuAddr, cl, wl, zeroExtend, vif1_regs.row);
            if (!unpackDone && m_vu1Data && totalBytes > 0 && pos + totalBytes <= sizeBytes)
            {
                const uint8_t *srcBase = data + pos;
                uint32_t srcIndex = 0u;
                for (uint32_t writeIndex = 0; writeIndex < writeVectorCount; ++writeIndex)
                {
                    const uint32_t cyclePos = writeIndex % wl;
                    const bool sourceAvailable = (cl >= wl) || (cyclePos < cl);

                    uint32_t destVec = 0;
                    if (cl >= wl)
                    {
                        destVec = (vuAddr + (writeIndex / wl) * cl + cyclePos) & 0x3FFu;
                    }
                    else
                    {
                        destVec = (vuAddr + writeIndex) & 0x3FFu;
                    }

                    uint32_t destOff = destVec * 16u;
                    if (destOff + 16u > PS2_VU1_DATA_SIZE)
                    {
                        if (sourceAvailable && srcIndex < sourceVectorCount)
                            ++srcIndex;
                        continue;
                    }

                    uint32_t lanes[4] = {0u, 0u, 0u, 0u};
                    std::memcpy(lanes, m_vu1Data + destOff, sizeof(lanes));
                    uint32_t decompressed[4] = {lanes[0], lanes[1], lanes[2], lanes[3]};
                    bool decoded = false;

                    const uint8_t *srcVec = nullptr;
                    if (sourceAvailable && srcIndex < sourceVectorCount)
                    {
                        srcVec = srcBase + srcIndex * bytesPerVector;
                        ++srcIndex;
                        decoded = true;
                    }

                    auto extend16 = [&](uint16_t raw) -> uint32_t
                    {
                        if (zeroExtend)
                            return static_cast<uint32_t>(raw);
                        return static_cast<uint32_t>(static_cast<int32_t>(static_cast<int16_t>(raw)));
                    };

                    auto extend8 = [&](uint8_t raw) -> uint32_t
                    {
                        if (zeroExtend)
                            return static_cast<uint32_t>(raw);
                        return static_cast<uint32_t>(static_cast<int32_t>(static_cast<int8_t>(raw)));
                    };

                    bool handledFormat = true;
                    if (!decoded)
                    {
                        handledFormat = false;
                    }
                    else if (vl == 0u)
                    {
                        if (components == 1)
                        {
                            uint32_t scalar = 0;
                            std::memcpy(&scalar, srcVec, sizeof(scalar));
                            decompressed[0] = scalar;
                            decompressed[1] = scalar;
                            decompressed[2] = scalar;
                            decompressed[3] = scalar;
                        }
                        else
                        {
                            const uint32_t limit = (components > 4) ? 4u : static_cast<uint32_t>(components);
                            for (uint32_t c = 0; c < limit; ++c)
                            {
                                uint32_t scalar = 0;
                                std::memcpy(&scalar, srcVec + c * 4u, sizeof(scalar));
                                decompressed[c] = scalar;
                            }
                        }
                    }
                    else if (vl == 1u)
                    {
                        if (components == 1)
                        {
                            uint16_t raw = 0;
                            std::memcpy(&raw, srcVec, sizeof(raw));
                            const uint32_t scalar = extend16(raw);
                            decompressed[0] = scalar;
                            decompressed[1] = scalar;
                            decompressed[2] = scalar;
                            decompressed[3] = scalar;
                        }
                        else
                        {
                            const uint32_t limit = (components > 4) ? 4u : static_cast<uint32_t>(components);
                            for (uint32_t c = 0; c < limit; ++c)
                            {
                                uint16_t raw = 0;
                                std::memcpy(&raw, srcVec + c * 2u, sizeof(raw));
                                decompressed[c] = extend16(raw);
                            }
                        }
                    }
                    else if (vl == 2u)
                    {
                        if (components == 1)
                        {
                            const uint32_t scalar = extend8(srcVec[0]);
                            decompressed[0] = scalar;
                            decompressed[1] = scalar;
                            decompressed[2] = scalar;
                            decompressed[3] = scalar;
                        }
                        else
                        {
                            const uint32_t limit = (components > 4) ? 4u : static_cast<uint32_t>(components);
                            for (uint32_t c = 0; c < limit; ++c)
                            {
                                decompressed[c] = extend8(srcVec[c]);
                            }
                        }
                    }
                    else if (vl == 3u && vn == 3u)
                    {
                        // V4-5 expands RGB to bits 3..7 and alpha to bit 7.
                        uint16_t packed = 0;
                        std::memcpy(&packed, srcVec, sizeof(packed));
                        decompressed[0] = (packed & 0x001Fu) << 3u;
                        decompressed[1] = (packed & 0x03E0u) >> 2u;
                        decompressed[2] = (packed & 0x7C00u) >> 7u;
                        decompressed[3] = (packed & 0x8000u) >> 8u;
                    }
                    else
                    {
                        handledFormat = false;
                    }

                    // V2 writes XYXY on hardware, before masks and STMOD apply.
                    if (handledFormat && components == 2)
                    {
                        decompressed[2] = decompressed[0];
                        decompressed[3] = decompressed[1];
                    }

                    // Unknown compressed format fallback: preserve legacy raw-copy behavior.
                    if (!handledFormat && decoded && !maskEnable && (vif1_regs.mode == 0u || vif1_regs.mode == 3u))
                    {
                        uint32_t copyBytes = (bytesPerVector < 16u) ? bytesPerVector : 16u;
                        std::memcpy(m_vu1Data + destOff, srcVec, copyBytes);
                        continue;
                    }

                    const bool canAdd = (vl != 3u || vn != 3u);
                    const uint32_t mode = vif1_regs.mode & 3u;
                    const uint32_t colIdx = (cyclePos > 3u) ? 3u : cyclePos;
                    const uint32_t maskCycle = (cyclePos > 3u) ? 3u : cyclePos;

                    // GOW-Port: without mask, with data and without adding the row (STMOD 0/3), each component is the
                    // decompressed value: the per-component loop would write the same.
                    if (!maskEnable && decoded && handledFormat && (!canAdd || mode == 0u))
                    {
                        std::memcpy(m_vu1Data + destOff, decompressed, sizeof(decompressed));
                        continue;
                    }

                    for (uint32_t field = 0u; field < 4u; ++field)
                    {
                        uint32_t maskSpec = 0u;
                        if (maskEnable)
                        {
                            const uint32_t shift = ((maskCycle * 4u) + field) * 2u;
                            maskSpec = (vif1_regs.mask >> shift) & 0x3u;
                        }

                        // In fill-write cycles with suspended source reads, treat raw-data selections as row-fill.
                        if (!decoded && maskSpec == 0u)
                            maskSpec = 1u;

                        uint32_t writeVal = lanes[field];
                        if (maskSpec == 0u)
                        {
                            if (handledFormat)
                            {
                                writeVal = decompressed[field];
                                if (canAdd && (mode == 1u || mode == 2u))
                                {
                                    writeVal = writeVal + vif1_regs.row[field];
                                    if (mode == 2u)
                                        vif1_regs.row[field] = writeVal;
                                }
                                else if (canAdd && mode == 3u)
                                {
                                    // GOW-Port: STMOD 3 also stores the data in ROW (PCSX2 Vif_Unpack.cpp setVifRow).
                                    vif1_regs.row[field] = writeVal;
                                }
                            }
                        }
                        else if (maskSpec == 1u)
                        {
                            writeVal = vif1_regs.row[field];
                        }
                        else if (maskSpec == 2u)
                        {
                            writeVal = vif1_regs.col[colIdx];
                        }
                        else
                        {
                            continue; // write-protect
                        }

                        lanes[field] = writeVal;
                    }

                    std::memcpy(m_vu1Data + destOff, lanes, sizeof(lanes));
                }
            }
            pos += totalBytes;

            if (pos > sizeBytes)
                break;
            continue;
        }
        else
        {
            // GOW-Port: locally saves the first invalid stream to rebuild the error without publishing game data.
            static bool streamSaved = false;
            if (!streamSaved)
                if (const char *dumpPath = std::getenv("GOW_VIF_BAD_DUMP"))
                    if (std::FILE *file = std::fopen(dumpPath, "wb"))
                    {
                        std::fwrite(data, 1u, sizeBytes, file);
                        std::fclose(file);
                        streamSaved = true;
                        std::fprintf(stderr, "[gow-vif] invalid stream saved to %s, command at 0x%08x\n", dumpPath, cmdOffset);
                    }
            static unsigned unknownCmds = 0;
            ++unknownCmds;
            // GOW-Port: the count goes on past the first 20 lines, so a report tells a one-off from a fault every frame.
            if (unknownCmds > 20u && (unknownCmds & (unknownCmds - 1u)) == 0u)
                std::fprintf(stderr, "[vif1] %u stream faults so far\n", unknownCmds);
            if (unknownCmds <= 20u)
            {
                std::fprintf(stderr, "[vif1] unhandled VIFcode opcode=%02x imm=%04x num=%u pos=%08x size=%08x\n",
                             opcode, imm, num, cmdOffset, sizeBytes);
                // GOW-Port: the first faults print the commands before them and the words around them without
                // GOW_VIF_DIAG: that is what tells which command left the stream misaligned. No game data beyond these
                // few words reaches the log.
                if (traceVif || unknownCmds <= 2u)
                {
                    const uint32_t count = std::min(commandCount, 8u);
                    for (uint32_t i = 0u; i < count; ++i)
                    {
                        const VifCommand &entry = commands[(commandCount - count + i) & 7u];
                        std::fprintf(stderr, "[vif1] history offset=%08x code=%08x\n", entry.offset, entry.code);
                    }
                    const uint32_t begin = cmdOffset >= 64u ? cmdOffset - 64u : 0u;
                    const uint32_t end = std::min(sizeBytes, cmdOffset + 68u);
                    for (uint32_t p = begin; p + 4u <= end; p += 4u)
                    {
                        uint32_t word = 0u;
                        std::memcpy(&word, data + p, sizeof(word));
                        std::fprintf(stderr, "[vif1] word offset=%08x value=%08x\n", p, word);
                    }
                }
            }
            // GOW-Port: a stream that stops here never reaches the MSKPATH3 that would unmask PATH3 again, so the texture
            // uploads queued behind the mask would wait for good and the GS would keep drawing with stale textures (the
            // likely cause of issue #34: blocky picture after a save until the game was restarted, with this fault in
            // every frame). The rest of this stream is lost either way; the queued PATH3 packets are released so the next
            // frames upload their textures.
            if (m_path3Masked)
            {
                static unsigned released = 0;
                if (++released <= 5u)
                    std::fprintf(stderr, "[vif1] stream fault with PATH3 masked: unmasked, %zu queued packets released\n",
                                 m_path3MaskedFifo.size());
                m_path3Masked = false;
                flushMaskedPath3Packets();
            }
            // GOW-Port: a reserved VIF code is a stream fault, not a four-byte
            // no-op.  Continuing treated its payload as commands and could launch
            // VU1 with corrupt data, which then left the EE waiting indefinitely.
            break;
        }
    }
}
