// GOW-Port: loading one slot of the memory card from outside the game's own menus (the launcher's Saves page, the F2
// menu, GOW_LOAD_SLOT=<0..3> at start), during a session, without closing the game.
//
// Nothing here loads a save by itself: the request walks the path a player takes, and at each step writes only the value
// the game's own menu would write there. Addresses checked in SCUS_973.99 (docs/dev/MEMORY-CARD.md):
//   0x29E560  main state (jump table 0x2B7F80 of the dispatcher 0x21CF68): 3 main menu ready (0x21EBF8), 5 load screen
//             (0x21EDC0), 11 gameplay (0x21F3C8), 12 pause menu (0x21F638). 0x29E574 != 0: a change is pending.
//   0x21D868  back to the main menu (0x21E9D0 resets the session and sets state 1): what the pause menu's Quit runs
//             (0x21F638, action 2, after storing 0 in UI +0x1A8). Here it runs in place of one gameplay or pause frame.
//   *0x29CB94 the UI variables: +0x27C "the main menu made a choice", +0x278 the choice (1 new game, 2 load).
//   0x22F5E0  the load screen (state word 0x2A12B0, jump table 0x2B8500): state 8 waits for the slot the player picks in
//             0x2A1290 (-1 while none, 0 back, slot + 1), on the card 0x2A12A4, and then reads data<N>.bin.
//   0x35BE80  the slot index of each card as read from it (0x44 bytes per card, four records of 16 bytes:
//             payload number, 0xCAFEBAD1, play time float, save point u16, difficulty u8).
//   *0x29CBA0 / *0x29CBA4 / 0x29CBA8  the message bank (index {u16 id, u16 first line}, lines, index entries): the save
//             point's name is message 600 + save point and the difficulty's 3 + difficulty.
// Every step has a deadline; a request that cannot go on ends with a notice and leaves the game where it is.
// GOW_SAVE_DIAG=1 logs each step ([gow-load]).
#include "gow_game.h"
#include "gow_guest_functions.h"
#include "../lib/gow_notice.h"
#include "../lib/gow_save_backup.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

namespace gow_game
{
    namespace
    {
        constexpr uint32_t kMainState = 0x0029E560u, kStatePending = 0x0029E574u;
        constexpr uint32_t kUiVariables = 0x0029CB94u;
        constexpr uint32_t kUiQuitFlag = 0x1A8u, kUiChoice = 0x278u, kUiChoiceMade = 0x27Cu;
        constexpr uint32_t kGameplay = 0x0021F3C8u, kPause = 0x0021F638u, kMainMenu = 0x0021EBF8u;
        constexpr uint32_t kMenuReady = 0x0029CC54u; // 0x1BBCF8: the main menu's movie is loaded (set at 0x1BDB04)
        constexpr uint32_t kLoadState = 0x002A12B0u, kLoadPick = 0x002A1290u, kLoadCard = 0x002A12A4u;
        constexpr uint32_t kCardIndex = 0x0035BE80u, kIndexMagic = 0xCAFEBAD1u;
        constexpr uint32_t kPlayTime = 0x00302D0Cu; // the session's play time, set from the slot's record when it loads
        constexpr uint32_t kBankIndex = 0x0029CBA0u, kBankStrings = 0x0029CBA4u, kBankCount = 0x0029CBA8u;
        constexpr uint32_t kStateMainMenu = 3u, kStateLoadScreen = 5u, kStateGameplay = 11u, kStatePause = 12u;
        constexpr uint32_t kLoadPickSlot = 8u;

        enum class Phase
        {
            Idle,
            Leave,  // waiting for a gameplay or pause frame to go back to the main menu from
            Menu,   // waiting for the main menu, then choosing Load there
            Pick,   // waiting for the load screen to ask for a slot
            Loading // the slot was picked: waiting for gameplay
        };

        struct Request
        {
            Phase phase = Phase::Idle;
            int slot = -1;
            std::chrono::steady_clock::time_point deadline{};
        };
        Request g_request;

        bool diag()
        {
            static const bool on = [] { const char *v = std::getenv("GOW_SAVE_DIAG"); return v && v[0] == '1'; }();
            return on;
        }

        bool pt()
        {
            return gowNoticePortuguese();
        }

        void advance(Phase phase, int seconds)
        {
            g_request.phase = phase;
            g_request.deadline = std::chrono::steady_clock::now() + std::chrono::seconds(seconds);
        }

        void fail(const char *why)
        {
            std::fprintf(stderr, "[gow-load] slot %d not loaded: %s\n", g_request.slot + 1, why);
            gowNoticePost(pt() ? "Carregar save" : "Load save",
                          pt() ? "Não foi possível carregar o slot " + std::to_string(g_request.slot + 1) + "."
                               : "Slot " + std::to_string(g_request.slot + 1) + " could not be loaded.",
                          8.0, true);
            g_request = {};
        }

        bool plausible(uint32_t address)
        {
            const uint32_t physical = address & 0x01FFFFFFu;
            return physical >= 0x00100000u && physical < 0x02000000u;
        }

        // The first line of message `id` of the game's message bank, or "" when it is not loaded.
        std::string message(const uint8_t *rdram, unsigned id)
        {
            const uint32_t index = readGuest32(rdram, kBankIndex), strings = readGuest32(rdram, kBankStrings);
            const uint32_t count = readGuest32(rdram, kBankCount);
            if (!plausible(index) || !plausible(strings) || count == 0u || count > 4096u)
                return {};
            for (uint32_t i = 0; i < count; ++i) // 641 entries at most, a few times a second: a linear walk is enough
            {
                uint16_t entry[2];
                std::memcpy(entry, rdram + ((index + i * 4u) & 0x01FFFFFFu), sizeof(entry));
                if (entry[0] != id)
                    continue;
                const uint32_t line = readGuest32(rdram, strings + entry[1] * 4u);
                if (!plausible(line))
                    return {};
                const char *text = reinterpret_cast<const char *>(rdram + (line & 0x01FFFFFFu));
                return std::string(text, strnlen(text, 96));
            }
            return {};
        }

        // The game's own index of card 1 (filled once it checked the card). An entry per used slot, in its order.
        std::vector<GowSaveSlotInfo> cardSlots(const uint8_t *rdram)
        {
            std::vector<GowSaveSlotInfo> slots;
            for (int slot = 0; slot < 4; ++slot)
            {
                const uint32_t record = kCardIndex + static_cast<uint32_t>(slot) * 16u;
                if (readGuest32(rdram, record + 4u) != kIndexMagic || static_cast<int32_t>(readGuest32(rdram, record)) < 0)
                    continue;
                GowSaveSlotInfo info;
                info.slot = slot;
                uint16_t point = 0;
                std::memcpy(&point, rdram + record + 12u, sizeof(point));
                info.savePoint = point;
                info.difficulty = rdram[record + 14u];
                std::memcpy(&info.playSeconds, rdram + record + 8u, sizeof(float));
                info.place = point > 0u && point < 100u ? message(rdram, 600u + point) : std::string();
                info.difficultyName = info.difficulty < 4u ? message(rdram, 3u + info.difficulty) : std::string();
                slots.push_back(std::move(info));
            }
            return slots;
        }

        bool indexLoaded(const uint8_t *rdram)
        {
            for (int slot = 0; slot < 4; ++slot)
            {
                const uint32_t first = readGuest32(rdram, kCardIndex + static_cast<uint32_t>(slot) * 16u);
                if (first == 0xFFFFFFFFu || readGuest32(rdram, kCardIndex + slot * 16u + 4u) == kIndexMagic)
                    return true;
            }
            return false;
        }

        uint32_t uiVariables(const uint8_t *rdram)
        {
            const uint32_t ui = readGuest32(rdram, kUiVariables);
            return plausible(ui) ? ui & 0x01FFFFFFu : 0u;
        }

        void start(uint8_t *rdram, int slot)
        {
            if (g_request.phase != Phase::Idle && g_request.phase != Phase::Leave && g_request.phase != Phase::Menu)
            {
                std::fprintf(stderr, "[gow-load] slot %d ignored: a load is already under way\n", slot + 1);
                return;
            }
            g_request.slot = slot;
            if (indexLoaded(rdram))
            {
                const std::vector<GowSaveSlotInfo> slots = cardSlots(rdram);
                const auto found = std::find_if(slots.begin(), slots.end(), [slot](const GowSaveSlotInfo &s) { return s.slot == slot; });
                if (found == slots.end())
                {
                    fail("the slot is empty");
                    return;
                }
                const std::string place = found->place.empty() ? std::string() : " (" + found->place + ")";
                gowNoticePost(pt() ? "Carregar save" : "Load save",
                              (pt() ? "Carregando o slot " : "Loading slot ") + std::to_string(slot + 1) + place + ".", 6.0);
            }
            std::fprintf(stderr, "[gow-load] slot %d requested (main state %u)\n", slot + 1, readGuest32(rdram, kMainState));
            // generous first deadline: at start the game plays its intro before the main menu is ready
            advance(Phase::Leave, 120);
        }
    }

    // Pad read (EE thread, once per frame, gow_pad2.cpp): takes requests, publishes the slots for the F2 menu and makes
    // the menu choices of the walk.
    void gowSaveLoadPoll(uint8_t *rdram, R5900Context *ctx, PS2Runtime *)
    {
        static bool first = true;
        if (first)
        {
            first = false;
            if (const char *slot = std::getenv("GOW_LOAD_SLOT"); slot && slot[0] >= '0' && slot[0] <= '3' && slot[1] == '\0')
                gowSaveLoadRequest(slot[0] - '0');
        }
        static auto lastPublish = std::chrono::steady_clock::time_point{};
        const auto now = std::chrono::steady_clock::now();
        if (now - lastPublish >= std::chrono::seconds(1))
        {
            lastPublish = now;
            gowSaveSlotsPublish(cardSlots(rdram));
        }

        if (const int slot = gowSaveLoadTakeRequest(); slot >= 0)
            start(rdram, slot);
        if (g_request.phase == Phase::Idle)
            return;
        if (now > g_request.deadline)
        {
            fail("timed out");
            return;
        }
        const uint32_t state = readGuest32(rdram, kMainState);
        const bool pending = readGuest32(rdram, kStatePending) != 0u;
        if (diag())
        {
            static uint32_t lastState = 0xFFFFFFFFu, lastLoadState = 0xFFFFFFFFu;
            const uint32_t loadState = readGuest32(rdram, kLoadState);
            if (state != lastState || loadState != lastLoadState)
                std::fprintf(stderr, "[gow-load] phase %d: main state %u, load screen state %d, pick %d, menu ready %u\n",
                             static_cast<int>(g_request.phase), state, static_cast<int32_t>(loadState),
                             static_cast<int32_t>(readGuest32(rdram, kLoadPick)), readGuest32(rdram, kMenuReady));
            lastState = state;
            lastLoadState = loadState;
        }
        switch (g_request.phase)
        {
        case Phase::Leave:
            // gameplay and pause go back through their own frame (gowSaveLoadGameplay / gowSaveLoadPause); the menu and
            // the load screen are already on the way
            if (state == kStateMainMenu || state == kStateLoadScreen)
                advance(Phase::Menu, 60);
            break;
        case Phase::Menu:
            if (state == kStateLoadScreen)
            {
                if (diag())
                    std::fprintf(stderr, "[gow-load] load screen open\n");
                advance(Phase::Pick, 30);
            }
            else if (state == kStateMainMenu && readGuest32(rdram, kMenuReady) == 0u && GPR_U32(ctx, 2) != 0xFFFFFFFFu)
            {
                // the title screen ("press start") holds the main menu until Start is pressed: a short press every half
                // second, in the packet this pad read just returned (buttons are active-low, Start is bit 3 of byte 2)
                static unsigned frames = 0;
                if (uint8_t *packet = guestBuffer(rdram, GPR_U32(ctx, 5), 18); packet && frames++ % 30u < 6u)
                    packet[2] &= static_cast<uint8_t>(~0x08u);
            }
            // the choice itself is made in the main menu's own frame (gowSaveLoadMainMenu)
            break;
        case Phase::Pick:
            if (state != kStateLoadScreen)
            {
                fail("the load screen closed");
                break;
            }
            if (readGuest32(rdram, kLoadState) == kLoadPickSlot && readGuest32(rdram, kLoadPick) == 0xFFFFFFFFu &&
                readGuest32(rdram, kLoadCard) == 0u)
            {
                const uint32_t record = kCardIndex + static_cast<uint32_t>(g_request.slot) * 16u;
                if (readGuest32(rdram, record + 4u) != kIndexMagic || static_cast<int32_t>(readGuest32(rdram, record)) < 0)
                {
                    writeGuest32(rdram, kLoadPick, 0u); // back, as the player would from an empty slot
                    fail("the slot is empty");
                    break;
                }
                writeGuest32(rdram, kLoadPick, static_cast<uint32_t>(g_request.slot) + 1u);
                if (diag())
                    std::fprintf(stderr, "[gow-load] slot %d picked on the load screen\n", g_request.slot + 1);
                advance(Phase::Loading, 90);
            }
            break;
        case Phase::Loading:
            if (state == kStateGameplay && !pending)
            {
                float played = 0.0f;
                std::memcpy(&played, rdram + kPlayTime, sizeof(played));
                std::fprintf(stderr, "[gow-load] slot %d loaded (play time %s)\n", g_request.slot + 1, gowSavePlayTime(played).c_str());
                g_request = {};
            }
            else if (state == kStateMainMenu)
                fail("the game went back to the main menu");
            break;
        case Phase::Idle:
            break;
        }
    }

    namespace
    {
        // In place of one gameplay or pause frame: the pause menu's Quit (0x21D868 back to the main menu).
        bool leaveForLoad(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime, uint32_t entry)
        {
            if (ctx->pc != entry || g_request.phase != Phase::Leave || readGuest32(rdram, kStatePending) != 0u)
                return false;
            const uint32_t ui = uiVariables(rdram);
            if (!ui)
                return false;
            std::fprintf(stderr, "[gow-load] back to the main menu from state %u\n", readGuest32(rdram, kMainState));
            writeGuest32(rdram, ui + kUiQuitFlag, 0u);
            ctx->pc = 0x0021D868u; // a tail call: 0x21D868 returns to the dispatcher, as the handler would have
            sub_0021D868_0x21d868(rdram, ctx, runtime);
            advance(Phase::Menu, 60);
            return true;
        }
    }

    void gowSaveLoadGameplay(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (!leaveForLoad(rdram, ctx, runtime, kGameplay))
            sub_0021F3C8_0x21f3c8(rdram, ctx, runtime);
    }

    // The main menu's frame (0x21EBF8, state 3): the choice "Load", written right before the frame reads it, as the menu
    // does when the player picks it.
    void gowSaveLoadMainMenu(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (diag() && g_request.phase != Phase::Idle)
        {
            static unsigned calls = 0;
            if (calls++ % 120u == 0u)
                std::fprintf(stderr, "[gow-load] main menu frame #%u (phase %d, pc %x, menu ready %u)\n", calls,
                             static_cast<int>(g_request.phase), ctx->pc, readGuest32(rdram, kMenuReady));
        }
        // (0x29E574 stays set while the main menu is up, so unlike gameplay it does not mean "busy" here)
        if (ctx->pc == kMainMenu && g_request.phase == Phase::Menu && readGuest32(rdram, kMenuReady) != 0u)
            if (const uint32_t ui = uiVariables(rdram))
            {
                static int logged = 0;
                if (diag() && logged++ < 3)
                    std::fprintf(stderr, "[gow-load] main menu: choosing Load (menu ready %u)\n", readGuest32(rdram, kMenuReady));
                writeGuest32(rdram, ui + kUiChoice, 2u);
                writeGuest32(rdram, ui + kUiChoiceMade, 1u);
            }
        sub_0021EBF8_0x21ebf8(rdram, ctx, runtime);
    }

    void gowSaveLoadPause(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (!leaveForLoad(rdram, ctx, runtime, kPause))
            sub_0021F638_0x21f638(rdram, ctx, runtime);
    }
}
