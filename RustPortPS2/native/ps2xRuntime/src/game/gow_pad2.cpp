// GOW-Port: libpad2 / libvib replacements: the game reads the DualShock 2 through the host pad backend.

#include "gow_game.h"
#include "ps2_host_backend.h"
#include "gow_pad2_packet.h"

#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>

// GOW-Port: the launcher's widescreen switch (gow_integration.cpp). 0 = off: the game keeps its original 4:3 picture.
float gowWidescreenAspect();

namespace gow_game
{
    namespace
    {
        // libpad2 uses socket handles and an 18-byte payload, unlike libpad's
        // port/slot API and 32-byte status packet. God of War expects state 1 and
        // a button profile beginning with 0xff to identify a DualShock 2.
        struct GowPadSocket { bool open = false; uint32_t port = 0; };
        std::array<GowPadSocket, 2> g_padSockets{};

        GowPadSocket *padSocket(R5900Context *ctx)
        {
            const uint32_t handle = GPR_U32(ctx, 4);
            return handle < g_padSockets.size() && g_padSockets[handle].open ? &g_padSockets[handle] : nullptr;
        }

        // libvib on the DualShock 2's two actuators (profile 0x03): actuator 0 = small motor (1 bit), actuator 1 = large
        // motor (8 bits). The game (0x17C130) reads the profile once per connection and sends the packed data only when
        // it changes; the pad backend drives the host gamepad's rumble.
        constexpr uint8_t kVibProfile = 0x03u;

        void gowPad2ReadState(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
        {
            // GOW-Port: with the launcher's widescreen switch on (PC_WIDESCREEN=A) this is the data part of PCSX2's
            // SCUS-97399_D6385328.pnach "Widescreen 16:9": the game's own widescreen flag and three 16:9 aspect floats,
            // re-applied every frame like PCSX2 does (the runtime then writes the chosen aspect and reads the projection
            // factor at 0x7F00). With the switch off nothing is forced: the game keeps its original 4:3 mode, HUD and
            // framing. (The pnach's "enhanced" code rewrite of 0x169250-0x16944C is in games/scus-97399/gow-port.toml.)
            if (gowWidescreenAspect() > 0.0f)
            {
                auto put32 = [rdram](uint32_t addr, uint32_t value) { std::memcpy(rdram + addr, &value, sizeof(value)); };
                put32(0x0029CC90u, 1u);
                put32(0x002FD930u, 0x3FE38E39u);
                put32(0x002FD968u, 0x3FE38E39u);
                put32(0x0032F1B0u, 0x3FE38E39u);
            }
            auto *buffer = guestBuffer(rdram, GPR_U32(ctx, 5), 18);
            const auto *socket = padSocket(ctx);
            uint8_t state[32]{};
            if (!buffer || !socket || socket->port != 0 || !runtime ||
                !runtime->padBackend().readState(0, 0, state, sizeof(state))) { returnToCaller(ctx, 0xffffffffu); return; }
            // GOW-Port: keyboard sticks. Without a gamepad they set every axis (PSPadBackend uses the first available slot);
            // with one (issue #12: a listed device may never send input) a key only replaces the axis it moves, and the
            // fixed WASD/IJKL keys stay out when the launcher's key map (PC_KEYS) is in charge.
            const bool gamepad = IsGamepadAvailable(0) || IsGamepadAvailable(1) || IsGamepadAvailable(2) || IsGamepadAvailable(3);
            static const bool mapped = std::getenv("PC_KEYS") != nullptr;
            if (!gamepad || !mapped)
            {
                auto fixedAxis = [gamepad](uint8_t &axis, int low, int high)
                {
                    if (IsKeyDown(low))
                        axis = 0;
                    else if (IsKeyDown(high))
                        axis = 255;
                    else if (!gamepad)
                        axis = 128;
                };
                fixedAxis(state[6], KEY_A, KEY_D);
                fixedAxis(state[7], KEY_W, KEY_S);
                fixedAxis(state[4], KEY_J, KEY_L);
                fixedAxis(state[5], KEY_I, KEY_K);
            }
            // Opt-in smoke test: advance title/new-game prompts without host UI input.
            // Capture the GS's own presented pixels; normal play never injects input.
            if (padSmokeTestEnabled())
                padSmokeTest(rdram, ctx, runtime, state);
            // GOW-Port: Left / Right held in a menu repeat. The game's menus act on the press only (a slider moved one step per press
            // however long the button was held, so 50 to 100 on a 2 % slider took 25 presses). Outside gameplay (state 11) a
            // direction held for REPEAT_DELAY seconds is released and pressed again every REPEAT_PERIOD seconds: the d-pad bits and
            // the left stick's X axis, so keyboard, d-pad and stick all repeat. GOW_MENU_REPEAT=0 turns it off.
            {
                static const bool off = [] { const char *v = std::getenv("GOW_MENU_REPEAT"); return v && v[0] == '0'; }();
                constexpr double kRepeatDelay = 0.40, kRepeatPeriod = 0.12; // seconds before the first repeat, between repeats
                using Clock = std::chrono::steady_clock;
                static Clock::time_point since[2];
                static bool held[2];                                       // 0 left, 1 right
                const bool menu = !off && readGuest32(rdram, 0x29E560u) != 11u;
                const bool down[2] = {(state[2] & 0x80u) == 0u || state[6] < 64u, (state[2] & 0x20u) == 0u || state[6] > 192u};
                const Clock::time_point now = Clock::now();
                for (int i = 0; i < 2; ++i)
                {
                    if (!menu || !down[i])
                    {
                        held[i] = false;
                        continue;
                    }
                    if (!held[i])
                    {
                        held[i] = true;
                        since[i] = now;
                        continue;
                    }
                    const double t = std::chrono::duration<double>(now - since[i]).count();
                    if (t > kRepeatDelay && std::fmod(t - kRepeatDelay, kRepeatPeriod) >= kRepeatPeriod / 2.0)
                    {
                        state[2] |= i == 0 ? 0x80u : 0x20u;        // released for the second half of each period
                        if (state[6] < 64u || state[6] > 192u)
                            state[6] = 128u;
                    }
                }
            }
            const auto packet = gow_pad2::makePacket(state);
            // GOW-Port: GOW_PAD_LOG=1 prints the libpad2 packet every 120 reads (buttons are active-low).
            static const bool padLog = std::getenv("GOW_PAD_LOG") != nullptr;
            static unsigned padReads = 0;
            if (padLog && ++padReads % 120u == 0u)
            {
                std::fprintf(stderr, "[gow-pad2:packet]");
                for (uint8_t byte : packet)
                    std::fprintf(stderr, " %02x", byte);
                std::fprintf(stderr, "\n");
            }
            std::memcpy(buffer, packet.data(), packet.size());
            const uint16_t buttons = static_cast<uint16_t>(state[2] | (state[3] << 8));
            static unsigned reads = 0;
            static uint16_t lastButtons = 0xffff;
            ++reads;
            // GOW-Port: GOW_PAD_LOG=1 also logs the buttons whenever they change (diagnostic: never in normal play, it is one
            // stderr line per input change for the whole session).
            if (padLog && (reads <= 3 || buttons != lastButtons))
                std::fprintf(stderr, "[gow-pad2] read buttons=%04x sticks=%u,%u,%u,%u\n", buttons, buffer[2], buffer[3], buffer[4], buffer[5]);
            lastButtons = buttons;
            static const bool anmDiag = std::getenv("GOW_ANM_DIAG") != nullptr;
            if (anmDiag && readGuest32(rdram, 0x29E560u) == 4u && reads % 100 == 0)
            {
                const uint32_t card = readGuest32(rdram, 0x29BE50u);
                std::fprintf(stderr, "[gow-transition] padType=%u cardState=%u pause=%u,%u,%u,%u speed=%x\n",
                             readGuest32(rdram, 0x2FD9C8u), readGuest32(rdram, card + 0x278u),
                             readGuest32(rdram, 0x29E564u), readGuest32(rdram, 0x29E568u),
                             readGuest32(rdram, 0x29E56Cu), readGuest32(rdram, 0x29E570u), readGuest32(rdram, 0x32F1F0u));
            }
            returnToCaller(ctx, 18u);
        }
    }

    void gowPad2Init(uint8_t *, R5900Context *ctx, PS2Runtime *)
    {
        g_padSockets = {};
        returnToCaller(ctx, 1u);
    }

    void gowPad2CreateSocket(uint8_t *rdram, R5900Context *ctx, PS2Runtime *)
    {
        const uint8_t *params = guestBuffer(rdram, GPR_U32(ctx, 4), 12);
        uint32_t port = 0, slot = 0;
        if (params) { std::memcpy(&port, params + 4, 4); std::memcpy(&slot, params + 8, 4); }
        if (!params || port >= 2 || slot != 0 || (GPR_U32(ctx, 5) & 63u)) { returnToCaller(ctx, 0xffffffffu); return; }
        for (uint32_t handle = 0; handle < g_padSockets.size(); ++handle)
        {
            if (g_padSockets[handle].open) continue;
            g_padSockets[handle] = {true, port};
            std::fprintf(stderr, "[gow-pad2] socket=%u port=%u slot=%u\n", handle, port, slot);
            returnToCaller(ctx, handle);
            return;
        }
        returnToCaller(ctx, 0xffffffffu);
    }

    void gowPad2DeleteSocket(uint8_t *, R5900Context *ctx, PS2Runtime *)
    {
        auto *socket = padSocket(ctx);
        if (socket) socket->open = false;
        returnToCaller(ctx, socket ? 1u : 0xffffffffu);
    }

    void gowPad2GetState(uint8_t *, R5900Context *ctx, PS2Runtime *)
    {
        const auto *socket = padSocket(ctx);
        // Only the first port has a host input backend. Keep port 2 disconnected.
        returnToCaller(ctx, socket && socket->port == 0 ? 1u : 0u);
    }

    void gowPad2GetButtonProfile(uint8_t *rdram, R5900Context *ctx, PS2Runtime *)
    {
        auto *buffer = guestBuffer(rdram, GPR_U32(ctx, 5), 4);
        const auto *socket = padSocket(ctx);
        if (!buffer || !socket || socket->port != 0) { returnToCaller(ctx, 0xffffffffu); return; }
        std::memset(buffer, 0xff, 4);
        returnToCaller(ctx, 4u);
    }

    void gowPad2Read(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        gowPad2ReadState(rdram, ctx, runtime);
        gowWarpPoll(rdram, ctx, runtime);
        static const bool saveLoad = [] { const char *v = std::getenv("GOW_SAVE_LOAD"); return !v || v[0] != '0'; }();
        if (saveLoad)
            gowSaveLoadPoll(rdram, ctx, runtime);
    }

    // 0x27BF90 sceVibGetProfile(socket, u8 *profile) -> profile size in bytes
    void gowVibGetProfile(uint8_t *rdram, R5900Context *ctx, PS2Runtime *)
    {
        auto *profile = guestBuffer(rdram, GPR_U32(ctx, 5), 1);
        const auto *socket = padSocket(ctx);
        if (!profile || !socket || socket->port != 0) { returnToCaller(ctx, 0u); return; }
        profile[0] = kVibProfile;
        returnToCaller(ctx, 1u);
    }

    // 0x27C010 sceVibSetActParam(socket, profileSize, const u8 *profile, dataSize, const u8 *data) -> 1 = sent
    void gowVibSetActParam(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const auto *socket = padSocket(ctx);
        const uint32_t profileSize = GPR_U32(ctx, 5), dataSize = GPR_U32(ctx, 7);
        const auto *profile = guestBuffer(rdram, GPR_U32(ctx, 6), profileSize);
        const auto *data = guestBuffer(rdram, GPR_U32(ctx, 8), dataSize);
        if (!socket || socket->port != 0 || !runtime || !profile || !data || profileSize == 0u || dataSize > 32u)
        {
            returnToCaller(ctx, 0u);
            return;
        }
        // Data bits are packed LSB first in actuator order, for the actuators the profile selects.
        uint32_t bit = 0u;
        auto take = [&](uint32_t width) {
            uint32_t value = 0u;
            for (uint32_t i = 0; i < width && bit < dataSize * 8u; ++i, ++bit)
                value |= ((data[bit / 8u] >> (bit % 8u)) & 1u) << i;
            return value;
        };
        const bool small = (profile[0] & 1u) && take(1u) != 0u;
        const uint32_t large = (profile[0] & 2u) ? take(8u) : 0u;
        static const bool vibLog = std::getenv("GOW_VIB_LOG") != nullptr;
        if (vibLog)
            std::fprintf(stderr, "[gow-vib] small=%u large=%u\n", small ? 1u : 0u, large);
        runtime->padBackend().setActuators(0, small, static_cast<uint8_t>(large));
        returnToCaller(ctx, 1u);
    }
}
