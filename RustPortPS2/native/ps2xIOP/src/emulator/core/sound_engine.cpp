#include "sound_engine.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace ps2x::iop::detail
{
    namespace
    {
        constexpr int32_t kAdpcmFilters[5][2] = {{0, 0}, {60, 0}, {115, -52}, {98, -55}, {122, -60}};

        // libsd ids (entry >> 8), in the order of LIBSD's register table (LIBSD.IRX 0x4930, docs/dev/AUDIO-VIDEO.md)
        enum : uint32_t
        {
            IdVolL = 0x00, IdVolR = 0x01, IdPitch = 0x02, IdAdsr1 = 0x03, IdAdsr2 = 0x04, IdEnvx = 0x05, IdVolXL = 0x06,
            IdVolXR = 0x07, IdMmix = 0x08, IdMvolL = 0x09, IdMvolR = 0x0A, IdEvolL = 0x0B, IdEvolR = 0x0C, IdAvolL = 0x0D,
            IdAvolR = 0x0E, IdBvolL = 0x0F, IdBvolR = 0x10, IdMvolXL = 0x11, IdMvolXR = 0x12,
            IdPmon = 0x13, IdNon = 0x14, IdKon = 0x15, IdKoff = 0x16, IdEndx = 0x17, IdVmixL = 0x18, IdVmixEL = 0x19,
            IdVmixR = 0x1A, IdVmixER = 0x1B,
            IdEsa = 0x1C, IdEea = 0x1D, IdTsa = 0x1E, IdIrqa = 0x1F, IdSsa = 0x20, IdLsa = 0x21, IdNax = 0x22,
        };

        int16_t clamp16(int32_t value) noexcept
        {
            return static_cast<int16_t>(std::clamp(value, -32768, 32767));
        }

        // voice / core volume: fixed mode (bit 15 = 0) = 15-bit signed; sweep mode applies its target at once
        int32_t volumeOf(uint16_t value) noexcept
        {
            if ((value & 0x8000u) == 0u)
                return static_cast<int16_t>(value << 1u) >> 1; // -0x4000..0x3FFF
            return (value & 0x2000u) != 0u ? 0 : 0x3FFF;
        }

        uint32_t idOf(uint32_t entry) noexcept { return (entry >> 8u) & 0xFFu; }
        uint32_t coreOf(uint32_t entry) noexcept { return entry & 1u; }
        uint32_t voiceOf(uint32_t entry) noexcept { return (entry >> 1u) & 0x1Fu; }
    }

    SoundEngine::SoundEngine() : m_ram(RamHalfwords), m_ring(kRingFrames * 2u)
    {
        reset();
    }

    void SoundEngine::reset()
    {
        std::fill(m_ram.begin(), m_ram.end(), uint16_t{0});
        m_cores = {};
        m_stream = {};
        m_streamHalf = {};
        m_flush.store(true, std::memory_order_release); // the audio thread drops what is buffered on its next call
        m_lastCycle = 0;
        m_targetCycle = 0; // GOW-Port: nothing left to mix
    }

    void SoundEngine::initCore(uint32_t core)
    {
        flushPending();
        Core &c = m_cores[core & 1u];
        const std::array<Voice, VoicesPerCore> voices{};
        c = Core{};
        c.voices = voices;
        m_stream[core & 1u] = {};
    }

    // ---- parameters ----------------------------------------------------------------------------------------------

    void SoundEngine::setParam(uint32_t entry, uint16_t value)
    {
        flushPending();
        Core &c = m_cores[coreOf(entry)];
        const uint32_t voice = voiceOf(entry);
        // GOW_SND_REGLOG=1: core-level writes (MMIX, master/effect/input volumes) as the driver sets them
        static const bool regLog = std::getenv("GOW_SND_REGLOG") != nullptr;
        if (regLog && idOf(entry) >= IdMmix && idOf(entry) <= IdBvolR)
            std::fprintf(stderr, "[snd-reg] core=%u id=%02x value=%04x\n", coreOf(entry), idOf(entry), value);
        // GOW_SND_CALLLOG=1: voice volume / pitch writes that change the value
        static const bool callLog = std::getenv("GOW_SND_CALLLOG") != nullptr;
        if (callLog && voice < VoicesPerCore && idOf(entry) <= IdPitch)
        {
            const Voice &v = c.voices[voice];
            const uint16_t old = idOf(entry) == IdVolL ? v.volL : idOf(entry) == IdVolR ? v.volR : v.pitch;
            if (old != value)
                std::fprintf(stderr, "[snd-call] core=%u voice=%u id=%02x %04x -> %04x\n", coreOf(entry), voice, idOf(entry), old, value);
        }
        switch (idOf(entry))
        {
        case IdVolL: if (voice < VoicesPerCore) c.voices[voice].volL = value; break;
        case IdVolR: if (voice < VoicesPerCore) c.voices[voice].volR = value; break;
        case IdPitch: if (voice < VoicesPerCore) c.voices[voice].pitch = value; break;
        case IdAdsr1: if (voice < VoicesPerCore) c.voices[voice].adsr1 = value; break;
        case IdAdsr2: if (voice < VoicesPerCore) c.voices[voice].adsr2 = value; break;
        case IdMmix: c.mmix = value; break;
        case IdMvolL: c.mvolL = value; break;
        case IdMvolR: c.mvolR = value; break;
        case IdEvolL: c.evolL = value; break;
        case IdEvolR: c.evolR = value; break;
        case IdAvolL: c.avolL = value; break;
        case IdAvolR: c.avolR = value; break;
        case IdBvolL: c.bvolL = value; break;
        case IdBvolR: c.bvolR = value; break;
        default: break; // ENVX / VOLX / MVOLX are read-only
        }
    }

    uint16_t SoundEngine::getParam(uint32_t entry)
    {
        flushPending();
        const Core &c = m_cores[coreOf(entry)];
        const uint32_t voice = std::min(voiceOf(entry), VoicesPerCore - 1u);
        const Voice &v = c.voices[voice];
        switch (idOf(entry))
        {
        case IdVolL: return v.volL;
        case IdVolR: return v.volR;
        case IdPitch: return v.pitch;
        case IdAdsr1: return v.adsr1;
        case IdAdsr2: return v.adsr2;
        case IdEnvx: return static_cast<uint16_t>(v.envelope);
        case IdVolXL: return static_cast<uint16_t>(volumeOf(v.volL) << 1);
        case IdVolXR: return static_cast<uint16_t>(volumeOf(v.volR) << 1);
        case IdMmix: return c.mmix;
        case IdMvolL: return c.mvolL;
        case IdMvolR: return c.mvolR;
        case IdEvolL: return c.evolL;
        case IdEvolR: return c.evolR;
        case IdAvolL: return c.avolL;
        case IdAvolR: return c.avolR;
        case IdBvolL: return c.bvolL;
        case IdBvolR: return c.bvolR;
        case IdMvolXL: return static_cast<uint16_t>(volumeOf(c.mvolL) << 1);
        case IdMvolXR: return static_cast<uint16_t>(volumeOf(c.mvolR) << 1);
        default: return 0;
        }
    }

    // ---- switches (24-bit voice masks) ---------------------------------------------------------------------------

    void SoundEngine::setSwitch(uint32_t entry, uint32_t mask)
    {
        flushPending();
        Core &c = m_cores[coreOf(entry)];
        mask &= 0xFFFFFFu;
        static const bool callLog = std::getenv("GOW_SND_CALLLOG") != nullptr;
        if (callLog && (idOf(entry) == IdKon || idOf(entry) == IdKoff) && mask != 0u)
            std::fprintf(stderr, "[snd-call] core=%u %s %06x\n", coreOf(entry), idOf(entry) == IdKon ? "KON" : "KOFF", mask);
        switch (idOf(entry))
        {
        case IdPmon: c.pmon = mask; break;
        case IdNon: c.non = mask; break;
        case IdKon: keyOn(c, mask); break;
        case IdKoff: keyOff(c, mask); break;
        case IdEndx: c.endx = 0; break; // writing ENDX clears it
        case IdVmixL: c.vmixL = mask; break;
        case IdVmixEL: c.vmixEL = mask; break;
        case IdVmixR: c.vmixR = mask; break;
        case IdVmixER: c.vmixER = mask; break;
        default: break;
        }
    }

    uint32_t SoundEngine::getSwitch(uint32_t entry)
    {
        flushPending();
        const Core &c = m_cores[coreOf(entry)];
        switch (idOf(entry))
        {
        case IdPmon: return c.pmon;
        case IdNon: return c.non;
        case IdEndx: return c.endx;
        case IdVmixL: return c.vmixL;
        case IdVmixEL: return c.vmixEL;
        case IdVmixR: return c.vmixR;
        case IdVmixER: return c.vmixER;
        default: return 0; // KON / KOFF read back as 0
        }
    }

    // ---- addresses (libsd: byte addresses, 16-byte aligned; stored as halfwords) ---------------------------------

    void SoundEngine::setAddr(uint32_t entry, uint32_t byteAddress)
    {
        flushPending();
        Core &c = m_cores[coreOf(entry)];
        const uint32_t halfwords = (byteAddress >> 1u) & (RamHalfwords - 1u) & ~7u;
        const uint32_t voice = voiceOf(entry);
        switch (idOf(entry))
        {
        case IdEsa: c.esa = halfwords; break;
        case IdEea: c.eea = halfwords; break;
        case IdTsa: c.tsa = halfwords; break;
        case IdIrqa: c.irqa = halfwords; break;
        case IdSsa:
            if (voice < VoicesPerCore)
                c.voices[voice].ssa = halfwords;
            break;
        case IdLsa:
            if (voice < VoicesPerCore)
            {
                c.voices[voice].lsa = halfwords;
                c.voices[voice].lsaFromDriver = true; // the driver's loop start wins over block flags
            }
            break;
        case IdNax:
            if (voice < VoicesPerCore)
            {
                Voice &v = c.voices[voice];
                v.nax = halfwords;
                v.playBlock = halfwords; // reads back as written until the next block is decoded
                v.playStarted = false;
            }
            break;
        default: break;
        }
    }

    uint32_t SoundEngine::getAddr(uint32_t entry)
    {
        flushPending();
        const Core &c = m_cores[coreOf(entry)];
        const Voice &v = c.voices[std::min(voiceOf(entry), VoicesPerCore - 1u)];
        uint32_t halfwords = 0;
        switch (idOf(entry))
        {
        case IdEsa: halfwords = c.esa; break;
        case IdEea: halfwords = c.eea; break;
        case IdTsa: halfwords = c.tsa; break;
        case IdIrqa: halfwords = c.irqa; break;
        case IdSsa: halfwords = v.ssa; break;
        case IdLsa: halfwords = v.lsa; break;
        case IdNax: halfwords = naxOf(v); break;
        default: break;
        }
        return halfwords << 1u;
    }

    // NAX as PCSX2 reports NextA: always inside the block being played (block + 1 .. block + 7). 'nax' is the next block
    // to decode; during the last block of a streaming half it already lies past the half, and 989snd (989NOMID.IRX
    // 0x13258) then aborts the stream (error 0x35): the music restarted every few seconds.
    uint32_t SoundEngine::naxOf(const Voice &v) noexcept
    {
        if (!v.playStarted)
            return (v.playBlock | 1u) & (RamHalfwords - 1u);
        const uint32_t played = std::min<uint32_t>(v.decodedIndex, 27u) / 4u; // 0..6 (4 samples per halfword)
        return (v.playBlock + 1u + played) & (RamHalfwords - 1u);
    }

    // sceSdSetCoreAttr ids (entry & ~1): 2 effect enable, 4 IRQ enable, 6 mute, 8 noise clock, 0xA SPDIF mode
    void SoundEngine::setCoreAttr(uint32_t entry, uint32_t value)
    {
        flushPending();
        Core &c = m_cores[coreOf(entry)];
        switch (entry & ~1u)
        {
        case 2: c.attr = static_cast<uint16_t>((c.attr & ~0x0080u) | (value ? 0x0080u : 0u)); break;
        case 4: c.attr = static_cast<uint16_t>((c.attr & ~0x0040u) | (value ? 0x0040u : 0u)); break;
        case 6: c.attr = static_cast<uint16_t>((c.attr & ~0x4000u) | (value ? 0x4000u : 0u)); break;
        case 8: c.attr = static_cast<uint16_t>((c.attr & ~0x3F00u) | ((value & 0x3Fu) << 8u)); break;
        default: break;
        }
    }

    uint32_t SoundEngine::getCoreAttr(uint32_t entry)
    {
        flushPending();
        const Core &c = m_cores[coreOf(entry)];
        switch (entry & ~1u)
        {
        case 2: return (c.attr >> 7u) & 1u;
        case 4: return (c.attr >> 6u) & 1u;
        case 6: return (c.attr >> 14u) & 1u;
        case 8: return (c.attr >> 8u) & 0x3Fu;
        default: return 0u;
        }
    }

    // ---- sample RAM --------------------------------------------------------------------------------------------------

    void SoundEngine::writeSampleRam(uint32_t byteAddress, const uint8_t *source, size_t bytes)
    {
        flushPending();
        uint32_t halfword = byteAddress >> 1u;
        for (size_t i = 0; i + 1u < bytes; i += 2u)
            m_ram[halfword++ & (RamHalfwords - 1u)] = static_cast<uint16_t>(source[i] | (source[i + 1u] << 8u));
    }

    void SoundEngine::readSampleRam(uint32_t byteAddress, uint8_t *destination, size_t bytes)
    {
        flushPending();
        uint32_t halfword = byteAddress >> 1u;
        for (size_t i = 0; i + 1u < bytes; i += 2u)
        {
            const uint16_t value = m_ram[halfword++ & (RamHalfwords - 1u)];
            destination[i] = static_cast<uint8_t>(value);
            destination[i + 1u] = static_cast<uint8_t>(value >> 8u);
        }
    }

    void SoundEngine::clearSampleRam(uint32_t byteAddress, size_t bytes)
    {
        flushPending();
        uint32_t halfword = byteAddress >> 1u;
        for (size_t i = 0; i + 1u < bytes; i += 2u)
            m_ram[halfword++ & (RamHalfwords - 1u)] = 0u;
    }

    // ---- core input stream -------------------------------------------------------------------------------------------

    void SoundEngine::startStream(uint32_t core, const uint8_t *ring, uint32_t size, bool loop)
    {
        flushPending();
        StreamRing &s = m_stream[core & 1u];
        s.loop = loop;
        s.base = ring;
        s.size = size & ~0x3FFu; // whole 1 KB L/R block pairs
        s.position = 0;
        s.active = ring != nullptr && s.size != 0u;
        m_streamHalf[core & 1u] = false;
    }

    void SoundEngine::stopStream(uint32_t core)
    {
        flushPending();
        m_stream[core & 1u].active = false;
    }

    bool SoundEngine::takeStreamHalf(uint32_t core) noexcept
    {
        flushPending();
        const bool half = m_streamHalf[core & 1u];
        m_streamHalf[core & 1u] = false;
        return half;
    }

    // One sample per channel from the ring: blocks of 256 left samples (512 bytes) then 256 right samples.
    void SoundEngine::streamSample(uint32_t core, int32_t &left, int32_t &right)
    {
        StreamRing &s = m_stream[core];
        if (!s.active)
            return;
        const uint32_t block = s.position & ~0x3FFu, index = (s.position & 0x3FFu) >> 1u; // sample 0..255 in the block
        int16_t l, r;
        std::memcpy(&l, s.base + block + index * 2u, 2u);
        std::memcpy(&r, s.base + block + 0x200u + index * 2u, 2u);
        const Core &c = m_cores[core];
        // BVOL (sound-data input volume) is a plain 16-bit signed level (0x7FFF = 1.0), not the voice volume format
        // with fixed/sweep modes; 989snd sets 0x7FFE for the movie stream.
        const auto inputVolume = [](uint16_t v) { return static_cast<int32_t>(static_cast<int16_t>(v)) >> 1; };
        if ((c.mmix & 0x0080u) != 0u) // MMIX sound-input bits: the driver enables them while its stream runs
            left += (l * inputVolume(c.bvolL)) >> 14;
        if ((c.mmix & 0x0040u) != 0u)
            right += (r * inputVolume(c.bvolR)) >> 14;
        const uint32_t before = s.position;
        s.position = block + ((index + 1u) * 2u);
        if (index + 1u == 256u)
            s.position = block + 0x400u;
        if (s.position >= s.size)
        {
            s.position = 0;
            if (!s.loop)
                s.active = false;
        }
        const uint32_t half = s.size / 2u;
        if ((before < half && s.position >= half) || s.position == 0)
            m_streamHalf[core] = true;
    }

    // ---- voices ------------------------------------------------------------------------------------------------------

    void SoundEngine::keyOn(Core &c, uint32_t mask)
    {
        for (uint32_t index = 0; index < VoicesPerCore; ++index)
        {
            if ((mask & (1u << index)) == 0u)
                continue;
            Voice &v = c.voices[index];
            Voice fresh{};
            fresh.volL = v.volL;
            fresh.volR = v.volR;
            fresh.pitch = v.pitch;
            fresh.adsr1 = v.adsr1;
            fresh.adsr2 = v.adsr2;
            fresh.ssa = v.ssa;
            fresh.active = true;
            fresh.nax = v.ssa & ~7u;
            fresh.lsa = v.ssa & ~7u;
            fresh.playBlock = v.ssa & ~7u; // NAX reads SSA | 1 until the first block is decoded (PCSX2 Start())
            fresh.phase = Voice::Phase::Attack;
            v = fresh;
            c.endx &= ~(1u << index);
        }
    }

    void SoundEngine::keyOff(Core &c, uint32_t mask)
    {
        for (uint32_t index = 0; index < VoicesPerCore; ++index)
        {
            Voice &v = c.voices[index];
            if ((mask & (1u << index)) != 0u && v.active)
            {
                v.phase = Voice::Phase::Release;
                v.envelopeCounter = 0;
            }
        }
    }

    void SoundEngine::decodeBlock(Core &c, uint32_t index)
    {
        Voice &v = c.voices[index];
        // end of the previous block: with the end flag jump to LSA; without "repeat" the voice stops
        if ((v.blockFlags & 1u) != 0u)
        {
            c.endx |= 1u << index;
            v.nax = v.lsa;
            if ((v.blockFlags & 2u) == 0u)
            {
                v.active = false;
                v.phase = Voice::Phase::Off;
                v.envelope = 0;
            }
        }
        v.blockFlags = 0;
        if (!v.active)
            return;

        const uint32_t block = v.nax & (RamHalfwords - 1u);
        const uint16_t header = m_ram[block];
        uint32_t shift = header & 0xFu;
        if (shift > 12u)
            shift = 9u;
        const uint32_t filter = std::min<uint32_t>((header >> 4u) & 7u, 4u);
        v.blockFlags = static_cast<uint8_t>(header >> 8u);
        if ((v.blockFlags & 4u) != 0u && !v.lsaFromDriver)
            v.lsa = block;

        for (uint32_t word = 1; word < 8u; ++word)
        {
            const uint16_t packed = m_ram[(block + word) & (RamHalfwords - 1u)];
            for (uint32_t nibble = 0; nibble < 4u; ++nibble)
            {
                const int32_t raw = static_cast<int16_t>(((packed >> (nibble * 4u)) & 0xFu) << 12u) >> shift;
                const int32_t predicted = (v.hist1 * kAdpcmFilters[filter][0] + v.hist2 * kAdpcmFilters[filter][1] + 32) >> 6;
                const int16_t sample = clamp16(raw + predicted);
                v.hist2 = v.hist1;
                v.hist1 = sample;
                v.decoded[(word - 1u) * 4u + nibble] = sample;
            }
        }
        v.decodedIndex = 0;
        v.nax = (block + 8u) & (RamHalfwords - 1u);
        v.playBlock = block;
        v.playStarted = true;
    }

    void SoundEngine::stepEnvelope(Voice &v)
    {
        uint32_t rate = 0;
        bool exponential = false;
        bool increase = false;
        switch (v.phase)
        {
        case Voice::Phase::Attack:
            rate = (v.adsr1 >> 8u) & 0x7Fu;
            exponential = (v.adsr1 & 0x8000u) != 0u;
            increase = true;
            break;
        case Voice::Phase::Decay:
            rate = ((v.adsr1 >> 4u) & 0xFu) * 4u;
            exponential = true;
            break;
        case Voice::Phase::Sustain:
            rate = (v.adsr2 >> 6u) & 0x7Fu;
            exponential = (v.adsr2 & 0x8000u) != 0u;
            increase = (v.adsr2 & 0x4000u) == 0u;
            break;
        case Voice::Phase::Release:
            rate = (v.adsr2 & 0x1Fu) * 4u;
            exponential = (v.adsr2 & 0x20u) != 0u;
            break;
        case Voice::Phase::Off:
            return;
        }

        // psx-spx "SPU ADSR": cycles = 1 << max(0, shift - 11); step = value << max(0, 11 - shift)
        const int32_t shiftValue = static_cast<int32_t>(rate >> 2u);
        const int32_t stepValue = static_cast<int32_t>(rate & 3u);
        int32_t step = increase ? 7 - stepValue : -8 + stepValue;
        uint32_t cycles = 1u << static_cast<uint32_t>(std::max(0, shiftValue - 11));
        step <<= std::max(0, 11 - shiftValue);
        if (exponential && increase && v.envelope > 0x6000)
            cycles *= 4u;
        // GOW-Port: an arithmetic shift, as on the hardware (and in PCSX2): the division truncated toward zero, so for a small
        // |step * envelope| the step became 0 and the exponential release / decay stalled above 0 (a keyed-off voice never
        // went silent and stayed taken). The shift rounds toward minus infinity and always moves by at least 1.
        if (exponential && !increase)
            step = (step * v.envelope) >> 15;

        if (++v.envelopeCounter >= cycles)
        {
            v.envelopeCounter = 0;
            v.envelope = std::clamp(v.envelope + step, 0, 0x7FFF);
        }

        switch (v.phase)
        {
        case Voice::Phase::Attack:
            if (v.envelope >= 0x7FFF)
            {
                v.phase = Voice::Phase::Decay;
                v.envelopeCounter = 0;
            }
            break;
        case Voice::Phase::Decay:
            if (v.envelope <= static_cast<int32_t>(((v.adsr1 & 0xFu) + 1u) * 0x800u))
            {
                v.phase = Voice::Phase::Sustain;
                v.envelopeCounter = 0;
            }
            break;
        case Voice::Phase::Release:
            if (v.envelope <= 0)
            {
                v.envelope = 0;
                v.phase = Voice::Phase::Off;
                v.active = false;
            }
            break;
        default:
            break;
        }
    }

    int32_t SoundEngine::stepVoice(Core &c, uint32_t index)
    {
        Voice &v = c.voices[index];
        if (!v.active)
            return 0;
        // pitch 4.12 (0x1000 = 48 kHz), linear interpolation between the last two samples (the hardware uses a
        // 4-point gaussian)
        v.pitchCounter += std::min<uint32_t>(v.pitch, 0x3FFFu);
        while (v.pitchCounter >= 0x1000u && v.active)
        {
            v.pitchCounter -= 0x1000u;
            if (v.decodedIndex >= 28u)
                decodeBlock(c, index);
            if (!v.active)
                break;
            v.prev[1] = v.prev[0];
            v.prev[0] = v.decoded[v.decodedIndex++];
        }
        if (!v.active)
            return 0;
        const int32_t fraction = static_cast<int32_t>(v.pitchCounter & 0xFFFu);
        const int32_t sample = v.prev[1] + (((v.prev[0] - v.prev[1]) * fraction) >> 12);
        stepEnvelope(v);
        return clamp16((sample * v.envelope) >> 15);
    }

    void SoundEngine::step()
    {
        int32_t coreOut[2][2] = {};
        for (uint32_t core = 0; core < 2u; ++core)
        {
            Core &c = m_cores[core];
            int32_t left = 0, right = 0;
            for (uint32_t index = 0; index < VoicesPerCore; ++index)
            {
                const int32_t sample = stepVoice(c, index);
                if (sample == 0)
                    continue;
                if ((c.vmixL & (1u << index)) != 0u)
                    left += (sample * volumeOf(c.voices[index].volL)) >> 14;
                if ((c.vmixR & (1u << index)) != 0u)
                    right += (sample * volumeOf(c.voices[index].volR)) >> 14;
            }
            streamSample(core, left, right);
            coreOut[core][0] = left;
            coreOut[core][1] = right;
        }
        finishFrame(coreOut);
        // GOW_SND_VOICEDUMP=1: while a core input stream (a movie) plays, once per second for its first 10 seconds, every
        // voice that is still sounding (active, pitch and envelope non-zero) with its volumes and sample addresses
        static const bool voiceDump = std::getenv("GOW_SND_VOICEDUMP") != nullptr;
        if (voiceDump)
        {
            static uint64_t streamSamples = 0;
            const bool streaming = m_stream[0].active || m_stream[1].active;
            streamSamples = streaming ? streamSamples + 1u : 0u;
            if (streaming && streamSamples % 48000u == 1u && streamSamples < 48000u * 10u)
            {
                std::fprintf(stderr, "[snd-voices] t=%llus mvol0=%04x/%04x mvol1=%04x/%04x\n",
                             static_cast<unsigned long long>(streamSamples / 48000u), m_cores[0].mvolL, m_cores[0].mvolR,
                             m_cores[1].mvolL, m_cores[1].mvolR);
                for (uint32_t core = 0; core < 2u; ++core)
                    for (uint32_t index = 0; index < VoicesPerCore; ++index)
                    {
                        const Voice &v = m_cores[core].voices[index];
                        if (v.active && v.pitch != 0u && v.envelope != 0)
                            std::fprintf(stderr, "[snd-voices]   core=%u voice=%u pitch=%04x vol=%04x/%04x env=%d ssa=%x lsa=%x nax=%x\n",
                                         core, index, v.pitch, v.volL, v.volR, v.envelope, v.ssa * 2u, v.lsa * 2u, v.nax * 2u);
                    }
            }
        }
    }

    void SoundEngine::finishFrame(const int32_t coreOut[2][2])
    {
        // core 0 through its master volume, added to core 1's dry output, all through core 1's master volume
        // (simplified MMIX routing)
        const int32_t master[2][2] = {{volumeOf(m_cores[0].mvolL), volumeOf(m_cores[0].mvolR)},
                                      {volumeOf(m_cores[1].mvolL), volumeOf(m_cores[1].mvolR)}};
        int32_t sums[2];
        for (uint32_t side = 0; side < 2u; ++side)
        {
            // GOW-Port: 64-bit products: 24 voices at full scale sum to ~690000 and the int32 product with a master volume of
            // 0x3FFF overflowed (undefined behavior, a wrapped sign on MSVC). After the shift the result fits int32 again.
            const int32_t core0 = static_cast<int32_t>((static_cast<int64_t>(coreOut[0][side]) * master[0][side]) >> 14);
            sums[side] = static_cast<int32_t>((static_cast<int64_t>(core0 + coreOut[1][side]) * master[1][side]) >> 14);
        }
        // GOW-Port: output headroom. In a fight the mix exceeds full scale by up to ~6 dB in every build, and the limiter then
        // halves the gain for whole passages, which is heard as a blown-out, pumping sound. The output is lowered by 4 dB:
        // the limiter then only trims the loudest peaks a little (at most ~25 %), which is not heard. GOW_AUDIO_GAIN_DB=<dB> sets another level (0 = the previous loudness).
        static const int32_t outputGain = []
        {
            const char *v = std::getenv("GOW_AUDIO_GAIN_DB");
            const double db = v && *v ? std::clamp(std::atof(v), -30.0, 12.0) : -4.0;
            return static_cast<int32_t>(std::lround(16384.0 * std::pow(10.0, db / 20.0)));
        }();
        for (int32_t &sum : sums)
            sum = static_cast<int32_t>((static_cast<int64_t>(sum) * outputGain) >> 14);
        // GOW-Port: in a fight the mix often exceeds full scale (up to 3 % of the samples) and the hard clip is heard as a
        // blown-out sound. A peak limiter takes over only then: the gain drops at once to keep the peak at full scale and
        // returns to 1.0 in ~80 ms (stereo linked); below full scale the samples are untouched. GOW_AUDIO_LIMIT=0 restores
        // the hard clip.
        static const bool limiter = [] { const char *v = std::getenv("GOW_AUDIO_LIMIT"); return !(v && v[0] == '0'); }();
        const int32_t peak = std::max(std::abs(sums[0]), std::abs(sums[1]));
        if (peak > 32767)
            m_clipped.fetch_add(1, std::memory_order_relaxed);
        if (limiter)
        {
            if (static_cast<float>(peak) * m_limiterGain > 32767.0f)
                m_limiterGain = 32767.0f / static_cast<float>(peak);
            else if (m_limiterGain < 1.0f)
                m_limiterGain = std::min(1.0f, m_limiterGain + 1.0f / 3840.0f);
            if (m_limiterGain < 1.0f)
            {
                // GOW_AUDIO_STATS: deepest gain reduction (x 65536) and frames played below 0.9 since the last report
                const uint32_t fixed = static_cast<uint32_t>(m_limiterGain * 65536.0f);
                if (fixed < m_limiterMin.load(std::memory_order_relaxed))
                    m_limiterMin.store(fixed, std::memory_order_relaxed);
                if (m_limiterGain < 0.9f)
                    m_limiterDeep.fetch_add(1, std::memory_order_relaxed);
            }
        }
        int16_t mixed[2];
        for (uint32_t side = 0; side < 2u; ++side)
            mixed[side] = limiter && m_limiterGain < 1.0f
                              ? clamp16(static_cast<int32_t>(std::lround(static_cast<float>(sums[side]) * m_limiterGain)))
                              : clamp16(sums[side]);
        pushFrame(mixed[0], mixed[1]);
        // GOW_AUDIO_RAW=<file>: the mixed samples as produced (s16le stereo 48 kHz), before the output's rate adaptation,
        // so a recording can be compared sample by sample with its source (tools/movies.py). Not written while
        // GOW_SND_VERIFY captures a comparison pass.
        static std::FILE *raw = [] { const char *path = std::getenv("GOW_AUDIO_RAW"); return path ? std::fopen(path, "wb") : nullptr; }();
        if (raw && m_capture == nullptr)
        {
            static std::vector<int16_t> pending;
            pending.insert(pending.end(), mixed, mixed + 2);
            if (pending.size() >= 8192u)
            {
                std::fwrite(pending.data(), sizeof(int16_t), pending.size(), raw);
                std::fflush(raw);
                pending.clear();
            }
        }
    }

    // GOW-Port: batch mixing (sound_engine.h, advanceTo). Same output as `frames` calls of step().
    void SoundEngine::mixFrames(uint64_t frames)
    {
        // GOW_SND_PERFRAME=1: the former per-sample mixing; GOW_SND_VOICEDUMP reads every voice once per sample, so it keeps
        // that path too.
        static const bool perFrame = std::getenv("GOW_SND_PERFRAME") != nullptr || std::getenv("GOW_SND_VOICEDUMP") != nullptr;
        static const bool verify = std::getenv("GOW_SND_VERIFY") != nullptr;
        if (perFrame)
        {
            for (uint64_t i = 0; i < frames; ++i)
                step();
            return;
        }
        while (frames != 0u)
        {
            const uint32_t n = static_cast<uint32_t>(std::min<uint64_t>(frames, kBatchFrames));
            if (verify)
                verifyBatch(n);
            else
                mixBatch(n);
            frames -= n;
        }
    }

    // A voice's samples depend only on its own state (decodeBlock reads the sample RAM, which only LIBSD calls change, and
    // those mix what is due first), so each voice runs its `frames` samples in one go; its dry contribution is an integer
    // sum, the same in any order. The input stream of each core is independent of the voices and of the other core. The
    // master volumes and the limiter keep their per-sample order in finishFrame.
    void SoundEngine::mixBatch(uint32_t frames)
    {
        int32_t dry[2][2][kBatchFrames] = {}; // [core][side][frame]
        for (uint32_t core = 0; core < 2u; ++core)
        {
            Core &c = m_cores[core];
            for (uint32_t index = 0; index < VoicesPerCore; ++index)
            {
                // An inactive voice gives 0 and changes nothing, and nothing can start it within the batch.
                if (!c.voices[index].active)
                    continue;
                const bool toLeft = (c.vmixL & (1u << index)) != 0u;
                const bool toRight = (c.vmixR & (1u << index)) != 0u;
                const int32_t volL = volumeOf(c.voices[index].volL);
                const int32_t volR = volumeOf(c.voices[index].volR);
                // GOW-Port: the voice's samples first, then the mix as plain array loops the compiler can vectorize. A zero
                // sample added (sample * vol) >> 14 == 0, so dropping the `sample == 0` skip changes nothing.
                int32_t samples[kBatchFrames];
                for (uint32_t f = 0; f < frames; ++f)
                    samples[f] = stepVoice(c, index);
                if (toLeft)
                    for (uint32_t f = 0; f < frames; ++f)
                        dry[core][0][f] += (samples[f] * volL) >> 14;
                if (toRight)
                    for (uint32_t f = 0; f < frames; ++f)
                        dry[core][1][f] += (samples[f] * volR) >> 14;
            }
            for (uint32_t f = 0; f < frames; ++f)
                streamSample(core, dry[core][0][f], dry[core][1][f]);
        }
        for (uint32_t f = 0; f < frames; ++f)
        {
            const int32_t coreOut[2][2] = {{dry[0][0][f], dry[0][1][f]}, {dry[1][0][f], dry[1][1][f]}};
            finishFrame(coreOut);
        }
    }

    // GOW_SND_VERIFY=1: the batch is mixed from a copy of the state, then the same samples per sample from the original
    // state; the outputs and the resulting voice states must match. The per-sample result is the one that stays.
    void SoundEngine::verifyBatch(uint32_t frames)
    {
        const auto cores = m_cores;
        const auto stream = m_stream;
        const auto streamHalf = m_streamHalf;
        const float limiterGain = m_limiterGain;
        std::vector<int16_t> batched, reference;
        batched.reserve(frames * 2u);
        reference.reserve(frames * 2u);

        m_capture = &batched;
        mixBatch(frames);
        const auto batchedCores = m_cores;
        const auto batchedStream = m_stream;

        m_cores = cores;
        m_stream = stream;
        m_streamHalf = streamHalf;
        m_limiterGain = limiterGain;
        m_capture = &reference;
        for (uint32_t f = 0; f < frames; ++f)
            step();
        m_capture = nullptr;

        static uint64_t checked = 0, differing = 0;
        checked += frames;
        bool same = batched == reference;
        for (uint32_t core = 0; core < 2u && same; ++core)
        {
            same = batchedCores[core].endx == m_cores[core].endx && batchedStream[core].position == m_stream[core].position;
            for (uint32_t index = 0; index < VoicesPerCore && same; ++index)
            {
                const Voice &a = batchedCores[core].voices[index], &b = m_cores[core].voices[index];
                same = a.active == b.active && a.envelope == b.envelope && a.nax == b.nax && a.pitchCounter == b.pitchCounter &&
                       a.decodedIndex == b.decodedIndex && a.phase == b.phase && a.envelopeCounter == b.envelopeCounter;
            }
        }
        if (!same && differing++ < 20u)
            std::fprintf(stderr, "[snd-verify] batch of %u frames differs from per-sample mixing\n", frames);
        if (checked / 48000u != (checked - frames) / 48000u && (checked / 48000u) % 60u == 0u)
            std::fprintf(stderr, "[snd-verify] %llu frames checked, %llu batches differ\n", static_cast<unsigned long long>(checked),
                         static_cast<unsigned long long>(differing));
        for (size_t i = 0; i + 1u < reference.size(); i += 2u)
            pushFrame(reference[i], reference[i + 1u]);
    }

    void SoundEngine::pushFrame(int16_t left, int16_t right) noexcept
    {
        if (m_capture != nullptr) // GOW-Port: GOW_SND_VERIFY comparison pass
        {
            m_capture->push_back(left);
            m_capture->push_back(right);
            return;
        }
        const uint64_t write = m_writeCount.load(std::memory_order_relaxed);
        if (write - m_readCount.load(std::memory_order_acquire) >= kRingFrames)
        {
            m_overflow.fetch_add(1, std::memory_order_relaxed); // nobody is listening (or the device stopped)
            return;
        }
        const size_t slot = static_cast<size_t>(write & (kRingFrames - 1u)) * 2u;
        m_ring[slot] = left;
        m_ring[slot + 1u] = right;
        m_writeCount.store(write + 1u, std::memory_order_release);
    }

    void SoundEngine::advanceTo(uint64_t iopCycle)
    {
        if (iopCycle <= m_targetCycle)
            return;
        m_targetCycle = iopCycle;
        if ((m_targetCycle - m_lastCycle) / CyclesPerSample >= kBatchFrames)
            flushPending();
    }

    void SoundEngine::flushPending()
    {
        if (m_targetCycle <= m_lastCycle)
            return;
        const uint64_t samples = (m_targetCycle - m_lastCycle) / CyclesPerSample;
        if (samples == 0u)
            return;
        mixFrames(samples);
        m_lastCycle += samples * CyclesPerSample;
        // GOW_SND_VOICE_DIAG=1: every 5 s of mixed audio, per core: voices taken (active), in release, in release with an
        // envelope below 0x2000 (the ones a stalled release leaves behind) and the most taken at the end of any call
        static const bool voiceDiag = std::getenv("GOW_SND_VOICE_DIAG") != nullptr;
        if (voiceDiag && samples != 0u)
        {
            static uint64_t totalSamples = 0, windowSamples = 0;
            static uint32_t peakActive[2] = {};
            uint32_t active[2] = {}, release[2] = {}, releaseLow[2] = {};
            for (uint32_t core = 0; core < 2u; ++core)
            {
                for (const Voice &v : m_cores[core].voices)
                {
                    if (!v.active)
                        continue;
                    ++active[core];
                    if (v.phase == Voice::Phase::Release)
                    {
                        ++release[core];
                        if (v.envelope < 0x2000)
                            ++releaseLow[core];
                    }
                }
                peakActive[core] = std::max(peakActive[core], active[core]);
            }
            totalSamples += samples;
            windowSamples += samples;
            if (windowSamples >= 240000u)
            {
                std::fprintf(stderr,
                             "[gow-voices] t=%llu core0 active=%u/%u release=%u release_low=%u peak_active=%u | core1 active=%u/%u "
                             "release=%u release_low=%u peak_active=%u\n",
                             static_cast<unsigned long long>(totalSamples / 48000u), active[0], VoicesPerCore, release[0],
                             releaseLow[0], peakActive[0], active[1], VoicesPerCore, release[1], releaseLow[1], peakActive[1]);
                windowSamples = 0;
                peakActive[0] = peakActive[1] = 0;
            }
        }
    }

    size_t SoundEngine::pendingFrames() const
    {
        return static_cast<size_t>(m_writeCount.load(std::memory_order_acquire) - m_readCount.load(std::memory_order_acquire));
    }

    // Builds the next grain of the stretcher (waveform-similarity overlap-add). The grain starts within kSeek frames of the
    // nominal position, at the point where it best continues the previous grain, and is added to the fading half of that
    // one with Hann windows, so the output is a seamless copy of the input played faster or slower without a change of
    // pitch. False when the ring does not hold enough frames yet.
    bool SoundEngine::stretchGrain(uint64_t written)
    {
        static const std::array<float, kGrain> window = []
        {
            std::array<float, kGrain> w{}; // periodic Hann: two grains half a grain apart add up to exactly 1
            for (size_t i = 0; i < w.size(); ++i)
                w[i] = static_cast<float>(0.5 - 0.5 * std::cos(6.283185307179586 * static_cast<double>(i) / static_cast<double>(kGrain)));
            return w;
        }();
        constexpr size_t mask = kRingFrames - 1u;
        Stretcher &s = m_stretch;
        const int64_t nominal = static_cast<int64_t>(s.nominal + 0.5);
        const int64_t low = std::max<int64_t>(nominal - kSeek, 0);
        const int64_t high = nominal + kSeek;
        if (static_cast<int64_t>(written) < high + kGrain)
            return false;
        const auto sample = [&](int64_t frame, size_t side) -> float
        { return static_cast<float>(m_ring[(static_cast<size_t>(frame) & mask) * 2u + side]); };

        int64_t start = nominal;
        if (s.primed)
        {
            // the natural continuation of the last grain is the reference; look for the splice that matches it best
            const int64_t natural = s.lastStart + kHop;
            std::array<float, 2 * kSeek + kHop> candidates;
            std::array<double, 2 * kSeek + kHop + 1> energy;
            std::array<float, kHop> reference;
            const int64_t span = high - low + kHop;
            energy[0] = 0.0;
            for (int64_t i = 0; i < span; ++i)
            {
                const float mono = 0.5f * (sample(low + i, 0) + sample(low + i, 1));
                candidates[static_cast<size_t>(i)] = mono;
                energy[static_cast<size_t>(i) + 1u] = energy[static_cast<size_t>(i)] + static_cast<double>(mono) * mono;
            }
            double referenceEnergy = 0.0;
            for (int64_t i = 0; i < kHop; ++i)
            {
                const float mono = 0.5f * (sample(natural + i, 0) + sample(natural + i, 1));
                reference[static_cast<size_t>(i)] = mono;
                referenceEnergy += static_cast<double>(mono) * mono;
            }
            // normalized correlation, a slight preference for the nominal position; coarse pass on every other lag and
            // sample, then the neighbours of the best one at full resolution
            const auto score = [&](int64_t lag, bool coarse)
            {
                const size_t first = static_cast<size_t>(lag);
                const size_t stride = coarse ? 2u : 1u;
                double dot = 0.0;
                for (size_t i = 0; i < static_cast<size_t>(kHop); i += stride)
                    dot += static_cast<double>(candidates[first + i]) * reference[i];
                if (coarse)
                    dot *= 2.0;
                const double e = energy[first + static_cast<size_t>(kHop)] - energy[first];
                const double ncc = dot / std::sqrt(referenceEnergy * e + 1.0e6);
                return ncc - 0.005 * static_cast<double>(std::llabs(low + lag - nominal)) / static_cast<double>(kSeek);
            };
            const int64_t lags = high - low;
            int64_t best = 0;
            double bestScore = -1.0e30;
            for (int64_t lag = 0; lag <= lags; lag += 2)
            {
                const double value = score(lag, true);
                if (value > bestScore)
                {
                    bestScore = value;
                    best = lag;
                }
            }
            const int64_t coarseBest = best;
            bestScore = -1.0e30;
            for (int64_t lag = std::max<int64_t>(coarseBest - 1, 0); lag <= std::min(coarseBest + 1, lags); ++lag)
            {
                const double value = score(lag, false);
                if (value > bestScore)
                {
                    bestScore = value;
                    best = lag;
                }
            }
            start = low + best;
        }

        for (int64_t i = 0; i < kHop; ++i)
        {
            const float rise = window[static_cast<size_t>(i)];
            s.ready.push_back(s.tail[static_cast<size_t>(i) * 2u] + sample(start + i, 0) * rise);
            s.ready.push_back(s.tail[static_cast<size_t>(i) * 2u + 1u] + sample(start + i, 1) * rise);
        }
        for (int64_t i = 0; i < kHop; ++i)
        {
            const float fall = window[static_cast<size_t>(kHop + i)];
            s.tail[static_cast<size_t>(i) * 2u] = sample(start + kHop + i, 0) * fall;
            s.tail[static_cast<size_t>(i) * 2u + 1u] = sample(start + kHop + i, 1) * fall;
        }
        s.lastStart = start;
        s.primed = true;
        s.tailValid = true;
        return true;
    }

    // The device asks for maxFrames. The guest produces at the pace of the emulation, which drifts from the device clock
    // (the title screen and menus run ~3.7 % ahead of the wall clock, a heavy fight falls behind it and hitches by tens of
    // milliseconds). Playing the samples as they arrive left the buffer empty in the second case (a gap of silence = a
    // pop) and cut or sped up the sound in the first. Now a standing buffer of about targetLatencyFrames is kept: the
    // tempo follows its level (a smoothed error, at most 0.6x-1.5x, changing 0.4 % per grain) through the stretcher, which
    // keeps the pitch. Only when the ring runs dry the last grain fades out and the buffer is rebuilt before the sound
    // returns; after a long stall of the device the buffer is cut back to the target.
    size_t SoundEngine::drainOutput(int16_t *stereo, size_t maxFrames, size_t targetLatencyFrames)
    {
        constexpr size_t mask = kRingFrames - 1u;
        constexpr double kLevelAlpha = 0.0686;     // 1 - exp(-kHop / (48000 * 0.15 s))
        constexpr double kMaxAheadFrames = 14400.0; // 0.3 s above the target
        Stretcher &s = m_stretch;
        uint64_t written = m_writeCount.load(std::memory_order_acquire);
        if (m_flush.exchange(false, std::memory_order_acq_rel))
        {
            s = Stretcher{};
            s.nominal = static_cast<double>(written);
            m_readCount.store(written, std::memory_order_release);
        }

        if (targetLatencyFrames == SIZE_MAX)
        {
            // no latency control: the samples as they are
            const uint64_t read = m_readCount.load(std::memory_order_relaxed);
            const size_t frames = std::min<size_t>(maxFrames, static_cast<size_t>(written - read));
            for (size_t i = 0; i < frames; ++i)
            {
                const size_t slot = static_cast<size_t>((read + i) & mask) * 2u;
                stereo[i * 2u] = m_ring[slot];
                stereo[i * 2u + 1u] = m_ring[slot + 1u];
            }
            m_readCount.store(read + frames, std::memory_order_release);
            s.nominal = static_cast<double>(read + frames);
            s.primed = false;
            return frames;
        }

        const double target = static_cast<double>(std::max<size_t>(targetLatencyFrames, 2560u));
        const size_t wanted = maxFrames * 2u;
        size_t starved = 0, jumps = 0;
        double minRate = s.rate, maxRate = s.rate, minLevel = 1.0e30, maxLevel = 0.0;
        while (s.ready.size() < wanted)
        {
            written = m_writeCount.load(std::memory_order_acquire);
            double level = static_cast<double>(written) - s.nominal;
            if (s.waiting)
            {
                if (level < target * 0.95)
                    break;
                s.waiting = false;
                s.levelInit = false;
            }
            if (level > target + kMaxAheadFrames)
            {
                s.nominal = static_cast<double>(written) - target; // the previous grain still fades out over the new one
                s.primed = false;
                s.levelInit = false;
                level = target;
                ++jumps;
            }
            if (!stretchGrain(written))
            {
                if (s.tailValid)
                    s.ready.insert(s.ready.end(), s.tail.begin(), s.tail.end()); // fade out
                s.tail.fill(0.0f);
                s.tailValid = false;
                s.primed = false;
                s.waiting = true;
                ++starved;
                break;
            }
            minLevel = std::min(minLevel, level);
            maxLevel = std::max(maxLevel, level);
            if (!s.levelInit)
            {
                s.levelAvg = level;
                s.levelInit = true;
            }
            else
                s.levelAvg += kLevelAlpha * (level - s.levelAvg);
            // the smoothed level sets the tempo, but only outside a dead band (-5 % .. +20 % of the target): inside it the
            // audio is copied as it is (tempo exactly 1.0). A hitch of the emulation (~75 ms) takes the level out of the band, the
            // tempo falls a few percent until the buffer is refilled and returns to 1.0 without overshoot (a reaction
            // faster than that made the tempo swing between 0.7x and 1.3x in the game)
            const double error = (s.levelAvg - target) / target;
            const double wantedRate = std::clamp(error > 0.20   ? 1.0 + 0.4 * (error - 0.20)
                                                 : error < -0.05 ? 1.0 + 0.6 * (error + 0.05)
                                                                 : 1.0,
                                                 0.85, 1.10);
            s.rate += std::clamp(wantedRate - s.rate, -0.006, 0.006);
            minRate = std::min(minRate, s.rate);
            maxRate = std::max(maxRate, s.rate);
            s.nominal += static_cast<double>(kHop) * s.rate;
            const int64_t keep = std::min<int64_t>(s.lastStart, static_cast<int64_t>(s.nominal + 0.5) - kSeek);
            if (keep > 0 && static_cast<uint64_t>(keep) > m_readCount.load(std::memory_order_relaxed))
                m_readCount.store(static_cast<uint64_t>(keep), std::memory_order_release);
        }

        const size_t frames = std::min(maxFrames, s.ready.size() / 2u);
        for (size_t i = 0; i < frames * 2u; ++i)
            stereo[i] = static_cast<int16_t>(std::clamp(std::lround(s.ready[i]), -32768L, 32767L));
        s.ready.erase(s.ready.begin(), s.ready.begin() + static_cast<std::ptrdiff_t>(frames * 2u));

        // GOW_AUDIO_STATS=1: every ~5 s, frames requested by the device, frames missing (silence), times the ring ran
        // dry, buffer cut backs, the tempo range (1.0 = as produced), the buffered level (input frames ahead of the
        // playback position), frames the ring dropped and output samples at full scale
        static const bool stats = std::getenv("GOW_AUDIO_STATS") != nullptr;
        if (stats)
        {
            static size_t requested = 0, missing = 0, starvedSum = 0, jumpSum = 0, calls = 0;
            static double rateLow = 10.0, rateHigh = 0.0, levelLow = 1.0e30, levelHigh = 0.0;
            requested += maxFrames;
            missing += maxFrames - frames;
            starvedSum += starved;
            jumpSum += jumps;
            rateLow = std::min(rateLow, minRate);
            rateHigh = std::max(rateHigh, maxRate);
            levelLow = std::min(levelLow, minLevel);
            levelHigh = std::max(levelHigh, maxLevel);
            ++calls;
            if (requested >= 240000u)
            {
                std::fprintf(stderr,
                             "[audio-stats] requested=%zu missing=%zu starved=%zu cutback=%zu calls=%zu tempo=%.3f-%.3f level=%.0f-%.0f "
                             "overflow=%llu over_full_scale=%llu limiter_min_gain=%.3f limiter_below_0.9=%llu\n",
                             requested, missing, starvedSum, jumpSum, calls, rateLow, rateHigh, levelLow, levelHigh,
                             static_cast<unsigned long long>(m_overflow.exchange(0, std::memory_order_relaxed)),
                             static_cast<unsigned long long>(m_clipped.exchange(0, std::memory_order_relaxed)),
                             static_cast<double>(m_limiterMin.exchange(65536u, std::memory_order_relaxed)) / 65536.0,
                             static_cast<unsigned long long>(m_limiterDeep.exchange(0, std::memory_order_relaxed)));
                requested = missing = starvedSum = jumpSum = calls = 0;
                rateLow = 10.0;
                rateHigh = 0.0;
                levelLow = 1.0e30;
                levelHigh = 0.0;
            }
        }
        return frames;
    }
}
