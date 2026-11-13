// GOW-Port: cheat menu (trainer) for God of War (SCUS-97399, NTSC-U). F2 or
// PC_CHEATS_SHORTCUT="7"/"9+11" opens it during gameplay;
// PC_CHEATS="god_mode,one_hit_kill,..." gives the initial state and PC_LANGUAGE (en/pt-BR) the language.
//
// Addresses taken from the executable (not from third-party codes):
//   0x302CC0  Kratos's state (what goes into the save game):
//     +0x5C float magic, +0x60 float Rage of the Gods meter (0..100, AddRage caps it at 100),
//     +0x68 u16 red orbs (SetOrbs, 0x1DB250, caps them at 60000),
//     +0x76 u8 health level (0..4), +0x77 u8 magic level (0..5) (0x1D8200: Gorgon Eyes / Phoenix Feathers),
//     +0x78..+0x7C u8 levels of Zeus's Fury, Medusa's Gaze, Poseidon's Rage, Army of Hades and Artemis's
//     Blade (0..2), +0x7D u8 Blades of Chaos level (0..4); 0x1D8CB8 treats those figures as complete,
//     +0x82 u16 items (0x04 Zeus, 0x08 Medusa, 0x10 Poseidon, 0x20 Hades, 0x40 Artemis, 0x100 trident).
//   0x3028E0  player settings: +0x00..+0x10 max health per level, +0x14..+0x28 max magic per level.
//   0x29CD58  player controller; its first field is Kratos (health at +0x178, max at +0x17C).
//   0x29CB94  UI variables (Costumes_Unlocked +0x1AC, HardMode_Unlocked +0x1B0).
//   0x2A12C8  completed playthroughs (bits 0-2 easy/normal/hard, 3 God Mode, 4 Challenge of the Gods): this
//     drives the extras, costumes and God Mode difficulty (0x1BA9B8, 0x22F4B0). Saved to the memory card.
//   0x1E0A80  sets a creature's health (a0, f12; clamps it between 0 and max). 0x1E0A98 subtracts damage (a0, f12)
//     and signals the thresholds it crosses (boss phases); ends up calling 0x1E0A80.
#include "gow_cheats.h"
#include "gow_persist.h" // GOW-Port: a change is written back for the launcher
#include "gow_notice.h"
#include "gow_save_backup.h" // GOW-Port: the Saves page loads a slot of the card
#include "ps2_runtime.h"
#include "ps2_runtime_macros.h"
#include "raylib.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <string>
#include <vector>

namespace
{
    enum Cheat : int
    {
        Immortal,
        OneHitKills,
        InfiniteMagic,
        InfiniteRage,
        MaxOrbs,
        AllUpgrades,
        AllPowers,
        MaxMeters,
        Extras,
        kNumCheats
    };
    // NOTE: these id strings are a public contract (PC_CHEATS env var value, gow-launcher/src/pages/CheatsPage.tsx's
    // `CHEATS` list, documented in docs/dev/GAMEPLAY-HOOKS.md) and must not change.
    constexpr const char *kIds[kNumCheats] = {"god_mode", "one_hit_kill", "infinite_magic", "infinite_rage", "max_orbs",
                                              "all_upgrades", "all_powers", "max_meters", "unlock_extras"};

    constexpr uint32_t kMask = 0x01FFFFFFu;
    constexpr uint32_t kState = 0x00302CC0u;
    constexpr uint32_t kSettings = 0x003028E0u;
    constexpr uint32_t kPlayerControl = 0x0029CD58u;
    constexpr uint32_t kUiVariables = 0x0029CB94u;
    constexpr uint32_t kCompletedPlaythroughs = 0x002A12C8u;
    constexpr uint32_t kSetHealth = 0x001E0A80u;
    constexpr uint32_t kSubtractHealth = 0x001E0A98u;

    std::array<std::atomic<bool>, kNumCheats> g_active{};
    std::atomic<bool> g_installed{false};
    std::atomic<bool> g_menuOpen{false};
    std::atomic<bool> g_blockAfterClose{false};
    struct CheatTriggerPart
    {
        int gamepad = 0;
    };
    struct CheatTrigger
    {
        CheatTriggerPart first;
        CheatTriggerPart second;
        bool latched = false;
        bool wasDown = false;
    };
    CheatTrigger g_trigger;

    PS2Runtime::RecompiledFunction g_setHealth = nullptr;
    PS2Runtime::RecompiledFunction g_subtractHealth = nullptr;

    bool triggerPartDown(const CheatTriggerPart &part)
    {
        return part.gamepad > 0 && IsGamepadAvailable(0) && IsGamepadButtonDown(0, static_cast<GamepadButton>(part.gamepad));
    }

    bool cheatTriggerDown()
    {
        if (!g_trigger.first.gamepad)
            return false;
        if (!triggerPartDown(g_trigger.first))
            return false;
        return !g_trigger.second.gamepad || triggerPartDown(g_trigger.second);
    }

    bool cheatTriggerPressed()
    {
        const bool down = cheatTriggerDown();
        const bool pressed = down && !g_trigger.wasDown;
        g_trigger.wasDown = down;
        return pressed && !g_trigger.latched;
    }

    CheatTriggerPart parseTriggerPart(const std::string &part)
    {
        CheatTriggerPart result;
        try
        {
            result.gamepad = std::clamp(std::stoi(part), 1, static_cast<int>(GAMEPAD_BUTTON_RIGHT_THUMB));
        }
        catch (...)
        {
            result = {};
        }
        return result;
    }

    void configureCheatTrigger()
    {
        g_trigger = {};
        const char *value = std::getenv("PC_CHEATS_SHORTCUT");
        if (!value || !*value)
            return;
        std::string text(value);
        const size_t plus = text.find('+');
        try
        {
            g_trigger.first = parseTriggerPart(text.substr(0, plus));
            if (plus != std::string::npos)
                g_trigger.second = parseTriggerPart(text.substr(plus + 1));
            if (g_trigger.second.gamepad == g_trigger.first.gamepad)
                g_trigger.second = {};
        }
        catch (...)
        {
            g_trigger = {};
        }
        if (g_trigger.first.gamepad)
            std::cerr << "[cheats] gamepad shortcut configured: "
                      << std::to_string(g_trigger.first.gamepad)
                      << (g_trigger.second.gamepad ? "+" : "")
                      << (g_trigger.second.gamepad ? std::to_string(g_trigger.second.gamepad) : "")
                      << std::endl;
    }

    bool active(Cheat cheat) { return g_active[cheat].load(std::memory_order_relaxed); }

    bool diagActive()
    {
        static const bool diag = []
        {
            const char *value = std::getenv("PC_CHEATS_DIAG");
            return value && std::strcmp(value, "1") == 0;
        }();
        return diag;
    }

    // PC_CHEATS_DIAG=1: the first calls to the health functions (creature, before, requested, after).
    void diagHealth(const char *function, uint32_t creature, uint32_t player, float before, float requested, float applied)
    {
        static std::atomic<unsigned> messages{0};
        if (!diagActive() || messages.fetch_add(1, std::memory_order_relaxed) >= 400u)
            return;
        std::fprintf(stderr, "[cheats:health] %s creature=%06x%s health=%.1f requested=%.1f -> %.1f\n", function, creature,
                     creature == player ? " (Kratos)" : "", before, requested, applied);
    }

    bool validAddress(uint32_t address, uint32_t bytes = 4u)
    {
        return address >= 0x00100000u && address + bytes <= PS2_RAM_SIZE && (address & 3u) == 0u;
    }

    template <typename T>
    T read(const uint8_t *rdram, uint32_t address)
    {
        T value{};
        std::memcpy(&value, rdram + address, sizeof(T));
        return value;
    }

    template <typename T>
    void write(uint8_t *rdram, uint32_t address, T value)
    {
        std::memcpy(rdram + address, &value, sizeof(T));
    }

    float maxHealthOfLevel(const uint8_t *rdram, unsigned level)
    {
        return read<float>(rdram, kSettings + 4u * std::min(level, 4u));
    }

    // Kratos: first field of the player controller. 0 if the pointers are not in the game's RAM.
    uint32_t kratos(const uint8_t *rdram)
    {
        const uint32_t control = read<uint32_t>(rdram, kPlayerControl) & kMask;
        if (!validAddress(control))
            return 0u;
        const uint32_t creature = read<uint32_t>(rdram, control) & kMask;
        return validAddress(creature, 0x180u) ? creature : 0u;
    }

    // Before writing to Kratos (outside his own code), his max health must be one from the level table:
    // this avoids touching an object already freed during a load.
    bool kratosPlausible(const uint8_t *rdram, uint32_t creature)
    {
        const float max = read<float>(rdram, creature + 0x17Cu);
        for (unsigned level = 0; level <= 4u; ++level)
            if (max > 0.0f && max == maxHealthOfLevel(rdram, level))
                return true;
        return false;
    }

    // 0x1E0A80 (a0 creature, f12 new health). The arguments are only touched on entry: if the game resumes the
    // function after a checkpoint (ctx->pc different from the entry), the original is called with no changes.
    void setHealth(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const bool immortal = active(Immortal), oneHit = active(OneHitKills);
        const uint32_t creature = GPR_U32(ctx, 4) & kMask;
        if (ctx->pc == kSetHealth && (immortal || oneHit || diagActive()) && validAddress(creature, 0x180u))
        {
            const float current = read<float>(rdram, creature + 0x178u), requested = ctx->f[12];
            const uint32_t player = kratos(rdram);
            if (ctx->f[12] < current && player != 0u)
            {
                if (creature == player && immortal)
                    ctx->f[12] = current;
                else if (creature != player && oneHit)
                    ctx->f[12] = 0.0f;
            }
            diagHealth("fijar", creature, player, current, requested, ctx->f[12]);
        }
        g_setHealth(rdram, ctx, runtime);
    }

    // 0x1E0A98 (a0 creature, f12 damage). With one hit the damage covers the whole health bar, so the game signals
    // every threshold it crosses (boss phases) just as if it were taken away gradually.
    void subtractHealth(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const bool immortal = active(Immortal), oneHit = active(OneHitKills);
        const uint32_t creature = GPR_U32(ctx, 4) & kMask;
        if (ctx->pc == kSubtractHealth && (immortal || oneHit || diagActive()) && ctx->f[12] > 0.0f &&
            validAddress(creature, 0x180u))
        {
            const uint32_t player = kratos(rdram);
            const float requested = ctx->f[12];
            if (player != 0u && creature == player && immortal)
                ctx->f[12] = 0.0f;
            else if (player != 0u && creature != player && oneHit)
                ctx->f[12] = std::max(ctx->f[12], read<float>(rdram, creature + 0x178u) + 1.0f);
            diagHealth("restar", creature, player, read<float>(rdram, creature + 0x178u), requested, ctx->f[12]);
        }
        g_subtractHealth(rdram, ctx, runtime);
    }

    void diagnostics(const uint8_t *rdram)
    {
        static unsigned vblanks = 0;
        if (!diagActive() || (++vblanks % 120u) != 0u)
            return;
        const uint32_t control = read<uint32_t>(rdram, kPlayerControl) & kMask;
        const uint32_t player = kratos(rdram);
        std::fprintf(stderr,
                     "[cheats] control=%06x kratos=%06x health=%.1f/%.1f plausible=%d magic=%.1f rage=%.1f(%.1f) orbs=%u"
                     "levels health=%u magic=%u zeus=%u medusa=%u poseidon=%u hades=%u artemis=%u blades=%u"
                     "items=%04x selected_magic=%u eyes=%u feathers=%u completed=%x table health=%.0f,%.0f,%.0f,%.0f,%.0f magic=%.0f,%.0f,%.0f,%.0f,%.0f,%.0f\n",
                     control, player, player ? read<float>(rdram, player + 0x178u) : -1.0f,
                     player ? read<float>(rdram, player + 0x17Cu) : -1.0f, player ? kratosPlausible(rdram, player) : 0,
                     read<float>(rdram, kState + 0x5Cu), read<float>(rdram, kState + 0x60u), read<float>(rdram, kState + 0x64u),
                     read<uint16_t>(rdram, kState + 0x68u), rdram[kState + 0x76u], rdram[kState + 0x77u],
                     rdram[kState + 0x78u], rdram[kState + 0x79u], rdram[kState + 0x7Au], rdram[kState + 0x7Bu],
                     rdram[kState + 0x7Cu], rdram[kState + 0x7Du], read<uint16_t>(rdram, kState + 0x82u), rdram[kState + 0x7Eu],
                     rdram[kState + 0x7Fu], rdram[kState + 0x80u], read<uint32_t>(rdram, kCompletedPlaythroughs),
                     read<float>(rdram, kSettings + 0x00u), read<float>(rdram, kSettings + 0x04u), read<float>(rdram, kSettings + 0x08u),
                     read<float>(rdram, kSettings + 0x0Cu), read<float>(rdram, kSettings + 0x10u), read<float>(rdram, kSettings + 0x14u),
                     read<float>(rdram, kSettings + 0x18u), read<float>(rdram, kSettings + 0x1Cu), read<float>(rdram, kSettings + 0x20u),
                     read<float>(rdram, kSettings + 0x24u), read<float>(rdram, kSettings + 0x28u));
    }

    // ---- Menu (main thread) ----

    struct Texts
    {
        const char *title;
        const char *subtitle;
        const char *navigateHelp;
        const char *closeHelp;
        const char *shortcutHelp;
        const char *name[kNumCheats];
        const char *description[kNumCheats];
        // GOW-Port: the Saves page (a slot of the card, loaded through the game's own Load)
        const char *tabCheats;
        const char *tabSaves;
        const char *savesTitle;
        const char *savesSubtitle;
        const char *savesEmpty;
        const char *slot;
        const char *load;
        const char *confirm;
        const char *confirmText;
        const char *savesHelp;
        const char *pageHelp;
    };

    const Texts kEnglish = {
        "CHEAT MENU",
        "Game input is paused while the menu is open",
        "↑↓ / D-pad select    Enter / A (✕) toggle",
        "F2 / Esc / B (○) close",
        "open/close",
        {"God mode", "One-hit kills", "Infinite magic", "Infinite Rage of the Gods", "Max red orbs", "All upgrades",
         "All powers and weapons", "Max health and magic", "Unlock all extras"},
        {"Kratos takes no damage; health stays full.",
         "Enemies and bosses die in a single hit.",
         "The magic meter never drains.",
         "Rage meter always full; never runs out while active.",
         "60000 red orbs (the game's cap).",
         "Blades of Chaos level 5; magic and Blade of Artemis level 3.",
         "Zeus's Fury, Medusa's Gaze, Poseidon's Rage, Army of Hades, Blade of Artemis, Poseidon's Trident.",
         "Every Gorgon Eye and Phoenix Feather upgrade.",
         "God Mode difficulty, costumes, videos, Challenge of the Gods. Permanent once you save."},
        "Cheats",
        "Saves",
        "LOAD A SAVE",
        "The saves on memory card 1, as the game lists them",
        "No save on the card yet (the list appears once the game has read the card).",
        "Slot",
        "LOAD",
        "CONFIRM",
        "Press again to load. Progress since your last save is lost.",
        "↑↓ / D-pad select    Enter / A (✕) load",
        "Tab / LB RB (L1 R1) page",
    };

    const Texts kPortuguesee = {
        "MENU DE EXTRAS",
        "Com o menu aberto, o jogo não recebe comandos",
        "↑↓ / direcional escolher    Enter / A (✕) ativar",
        "F2 / Esc / B (○) fechar",
        "abrir/fechar",
        {"Invencibilidade", "Matar com um golpe", "Magia infinita", "Fúria dos Deuses infinita", "Máximo de orbes vermelhos",
         "Todas as melhorias", "Todos os poderes e armas", "Vida e magia no máximo", "Desbloquear todos os extras"},
        {"Kratos não recebe dano e a vida fica sempre cheia.",
         "Inimigos e chefes morrem com um único golpe.",
         "A barra de magia não diminui.",
         "Barra de fúria sempre cheia; não se esgota enquanto ativa.",
         "60.000 orbes vermelhos (limite do jogo).",
         "Lâminas do Caos no nível 5; magias e Lâmina de Ártemis no nível 3.",
         "Fúria de Zeus, Olhar da Medusa, Ira de Poseidon, Exército de Hades, Lâmina de Ártemis, Tridente de Poseidon.",
         "Todas as melhorias de Olhos de Górgona e Penas de Fênix.",
         "Dificuldade Modo Deus, trajes, vídeos, Desafio dos Deuses. Fica permanente ao salvar."},
        "Extras",
        "Saves",
        "CARREGAR UM SAVE",
        "Os saves do cartão de memória 1, na ordem do jogo",
        "Nenhum save no cartão ainda (a lista aparece depois que o jogo lê o cartão).",
        "Slot",
        "CARREGAR",
        "CONFIRMAR",
        "Aperte de novo para carregar. O progresso desde o último save se perde.",
        "↑↓ / direcional escolher    Enter / A (✕) carregar",
        "Tab / LB RB (L1 R1) página",
    };

    const Texts &texts()
    {
        static const Texts *chosen = []
        {
            const char *language = std::getenv("PC_LANGUAGE");
            if (language && std::strncmp(language, "pt", 2) == 0)
                return &kPortuguesee;
            return &kEnglish;
        }();
        return *chosen;
    }

    // GOW-Port: the codes are the raylib buttons stored by the launcher.
    // Showing the real shortcut avoids the menu suggesting that only F2 closes it.
    const char *triggerButtonName(int button)
    {
        switch (button)
        {
        case 1: return "↑";
        case 2: return "→";
        case 3: return "↓";
        case 4: return "←";
        case 5: return "Y (△)";
        case 6: return "B (○)";
        case 7: return "A (✕)";
        case 8: return "X (□)";
        case 9: return "LB (L1)";
        case 10: return "LT (L2)";
        case 11: return "RB (R1)";
        case 12: return "RT (R2)";
        case 13: return "Select";
        case 15: return "Start";
        case 16: return "L3";
        case 17: return "R3";
        default: return nullptr;
        }
    }

    std::string triggerHelp()
    {
        const char *first = triggerButtonName(g_trigger.first.gamepad);
        if (!first)
            return {};
        std::string label = first;
        if (const char *second = triggerButtonName(g_trigger.second.gamepad))
            label += std::string(" + ") + second;
        return label;
    }

    struct Fonts
    {
        Font regular{};
        Font bold{};
        bool custom = false;
    };

    // System font with English and Portuguese characters; if none is found, raylib's own.
    Font loadFont(const std::vector<std::string> &paths)
    {
        std::vector<int> codepoints;
        for (int c = 32; c < 127; ++c)
            codepoints.push_back(c);
        for (int c = 160; c < 256; ++c)
            codepoints.push_back(c);
        for (int c : {0x2190, 0x2191, 0x2192, 0x2193, 0x2022, 0x2026})
            codepoints.push_back(c);
        for (const std::string &path : paths)
        {
            std::error_code error;
            if (path.empty() || !std::filesystem::is_regular_file(path, error))
                continue;
            Font font = LoadFontEx(path.c_str(), 64, codepoints.data(), static_cast<int>(codepoints.size()));
            if (font.texture.id != 0 && font.glyphCount > 0)
            {
                GenTextureMipmaps(&font.texture);
                SetTextureFilter(font.texture, TEXTURE_FILTER_TRILINEAR);
                return font;
            }
        }
        return Font{};
    }

    Fonts &fonts()
    {
        static Fonts f = []
        {
            Fonts out;
            const char *custom = std::getenv("PC_MENU_FONT");
            out.regular = loadFont({custom ? custom : "", "/usr/share/fonts/TTF/DejaVuSans.ttf",
                                       "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                                       "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                                       "/usr/share/fonts/liberation/LiberationSans-Regular.ttf", "C:/Windows/Fonts/seguisb.ttf",
                                       "C:/Windows/Fonts/segoeui.ttf", "C:/Windows/Fonts/arial.ttf"});
            out.bold = loadFont({"/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                                        "/usr/share/fonts/noto/NotoSans-Bold.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
                                        "/usr/share/fonts/liberation/LiberationSans-Bold.ttf", "C:/Windows/Fonts/segoeuib.ttf",
                                        "C:/Windows/Fonts/arialbd.ttf"});
            out.custom = out.regular.texture.id != 0;
            if (!out.custom)
                out.regular = GetFontDefault();
            if (out.bold.texture.id == 0)
                out.bold = out.regular;
            return out;
        }();
        return f;
    }

    void drawText(const Font &font, const char *text, float x, float y, float size, Color color)
    {
        DrawTextEx(font, text, Vector2{std::round(x), std::round(y)}, size, size * 0.02f, color);
    }

    float textWidth(const Font &font, const char *text, float size)
    {
        return MeasureTextEx(font, text, size, size * 0.02f).x;
    }

    // Truncates the description with "..." if it doesn't fit.
    std::string truncate(const Font &font, const char *text, float size, float maxWidth)
    {
        std::string s(text);
        if (textWidth(font, s.c_str(), size) <= maxWidth)
            return s;
        while (!s.empty())
        {
            do
                s.pop_back();
            while (!s.empty() && (static_cast<unsigned char>(s.back()) & 0xC0u) == 0x80u);
            if (!s.empty())
                s.pop_back();
            const std::string test = s + "\xE2\x80\xA6";
            if (textWidth(font, test.c_str(), size) <= maxWidth)
                return test;
        }
        return s;
    }

    // GOW-Port: parchment palette shared with gow-launcher/src/styles.css (ink on parchment, bronze rims, blood red).
    constexpr Color kParchmentTop{236, 221, 190, 255};
    constexpr Color kParchmentBottom{223, 203, 165, 255};
    constexpr Color kInk{42, 26, 16, 255};
    constexpr Color kInkSoft{61, 43, 27, 255};
    constexpr Color kInkMuted{71, 50, 31, 255};
    constexpr Color kBronze{95, 67, 38, 255};
    constexpr Color kBronzeLight{176, 138, 85, 255};
    constexpr Color kRed{163, 25, 15, 255};
    constexpr Color kRedDark{122, 19, 13, 255};
    constexpr Color kCream{243, 228, 195, 255};

    struct MenuState
    {
        int selected = 0;
        int closeKey = 0;
        bool cursorHidden = false;
        int page = 0;          // GOW-Port: 0 cheats, 1 saves
        int savesSelected = 0;
        int armed = -1;        // the save row waiting for its confirmation press
        double armedAt = 0.0;
    };
    MenuState g_menu;

    struct Geometry
    {
        float scale, x, y, w, header, row, footer, h;
    };

    Geometry geometry()
    {
        const float sw = static_cast<float>(GetScreenWidth()), sh = static_cast<float>(GetScreenHeight());
        Geometry g{};
        g.scale = std::clamp(sh / 1080.0f, 0.6f, 2.0f);
        g.w = std::min(sw - 32.0f, 820.0f * g.scale);
        g.header = 96.0f * g.scale;
        g.row = 64.0f * g.scale;
        g.footer = 68.0f * g.scale;
        g.h = std::min(sh - 32.0f, g.header + g.row * static_cast<float>(kNumCheats) + g.footer);
        g.x = std::floor((sw - g.w) * 0.5f);
        g.y = std::floor((sh - g.h) * 0.5f);
        return g;
    }

    // GOW-Port: the Saves page lists what the game holds of the card (published by src/game/gow_save_load.cpp).
    const std::vector<GowSaveSlotInfo> &menuSlots()
    {
        static std::vector<GowSaveSlotInfo> slots;
        static double refreshed = -1.0;
        if (GetTime() - refreshed > 0.25)
        {
            slots = gowSaveSlotsSnapshot();
            refreshed = GetTime();
        }
        return slots;
    }

    int rowCount()
    {
        return g_menu.page == 0 ? kNumCheats : static_cast<int>(menuSlots().size());
    }

    void close(bool escClosesGame, int key);

    // First press arms the row, a second one within four seconds asks the game for that slot and closes the menu.
    void activateSave(int row, bool escClosesGame)
    {
        const std::vector<GowSaveSlotInfo> &slots = menuSlots();
        if (row < 0 || row >= static_cast<int>(slots.size()))
            return;
        if (g_menu.armed != row || GetTime() - g_menu.armedAt > 4.0)
        {
            g_menu.armed = row;
            g_menu.armedAt = GetTime();
            return;
        }
        g_menu.armed = -1;
        const GowSaveSlotInfo &slot = slots[row];
        std::cerr << "[cheats] load of slot " << slot.slot + 1 << " asked from the menu" << std::endl;
        gowSaveLoadRequest(slot.slot);
        close(escClosesGame, 0);
        g_blockAfterClose.store(true, std::memory_order_release);
    }

    void setActive(int index, bool next)
    {
        g_active[index].store(next, std::memory_order_relaxed);
        std::cerr << "[cheats] " << kIds[index] << (next ? " active" : " disabled") << std::endl;
        gowPersistTouch(); // GOW-Port: the launcher keeps the list for the next session
    }

    void toggle(int index)
    {
        setActive(index, !g_active[index].load(std::memory_order_relaxed));
    }

    void open(bool escClosesGame)
    {
        (void)escClosesGame;
        g_menuOpen.store(true, std::memory_order_release);
        SetExitKey(KEY_NULL);
        g_menu.cursorHidden = IsCursorHidden();
        if (g_menu.cursorHidden)
            ShowCursor();
    }

    void close(bool escClosesGame, int key)
    {
        g_menuOpen.store(false, std::memory_order_release);
        g_menu.closeKey = key;
        g_blockAfterClose.store(key != 0, std::memory_order_release);
        if (escClosesGame)
            SetExitKey(KEY_ESCAPE);
        if (g_menu.cursorHidden)
            HideCursor();
    }
}

// Installs the health wrappers (God of War only) and reads the initial state from PC_CHEATS.
void gowCheatsInstall(PS2Runtime &runtime, const std::string &elfPath)
{
    if (std::filesystem::path(elfPath).filename() != "SCUS_973.99" || g_installed.load())
        return;
    // RetroAchievements Hardcore does not allow cheats: without the health wrappers the F2 menu is also disabled.
    const char *raActive = std::getenv("PC_RA_ENABLED");
    const char *raHardcore = std::getenv("PC_RA_HARDCORE");
    if (raActive && raHardcore && raActive[0] == '1' && raHardcore[0] == '1')
    {
        std::cerr << "[cheats] RetroAchievements Hardcore active; cheat menu disabled" << std::endl;
        return;
    }
    if (!runtime.hasFunction(kSetHealth) || !runtime.hasFunction(kSubtractHealth))
    {
        std::cerr << "[cheats] health functions missing; cheat menu disabled" << std::endl;
        return;
    }
    g_setHealth = runtime.lookupFunction(kSetHealth);
    g_subtractHealth = runtime.lookupFunction(kSubtractHealth);
    runtime.replaceFunction(kSetHealth, &setHealth);
    runtime.replaceFunction(kSubtractHealth, &subtractHealth);
    if (const char *list = std::getenv("PC_CHEATS"))
    {
        std::string value(list);
        size_t start = 0;
        while (start <= value.size())
        {
            const size_t end = std::min(value.find(',', start), value.size());
            const std::string id = value.substr(start, end - start);
            for (int i = 0; i < kNumCheats; ++i)
                if (id == kIds[i])
                    g_active[i].store(true, std::memory_order_relaxed);
            start = end + 1;
        }
    }
    configureCheatTrigger();
    g_installed.store(true);
    std::string activeList;
    for (int i = 0; i < kNumCheats; ++i)
        if (g_active[i].load())
            activeList += std::string(activeList.empty() ? "" : ",") + kIds[i];
    std::cerr << "[cheats] cheat menu ready (F2"
              << (g_trigger.first.gamepad ? " + gamepad shortcut" : "") << "); active: "
              << (activeList.empty() ? "none" : activeList) << std::endl;
}

// GOW-Port: public interface (gow_cheats.h). It shares the state of the F2 menu and honors the same availability
// rule: g_installed stays false under RetroAchievements Hardcore, so nothing here can change a cheat in that mode.
bool gowCheatsAvailable()
{
    return g_installed.load(std::memory_order_acquire);
}

int gowCheatsCount()
{
    return kNumCheats;
}

const char *gowCheatsId(int index)
{
    return index >= 0 && index < kNumCheats ? kIds[index] : nullptr;
}

bool gowCheatsGet(int index)
{
    return gowCheatsAvailable() && index >= 0 && index < kNumCheats && active(static_cast<Cheat>(index));
}

bool gowCheatsSet(int index, bool enabled)
{
    if (!gowCheatsAvailable() || index < 0 || index >= kNumCheats)
        return false;
    if (active(static_cast<Cheat>(index)) != enabled)
        setActive(index, enabled);
    return true;
}

// EE thread, at the start of every VBlank: values that are kept while the cheat is active.
void gowCheatsVBlank(uint8_t *rdram)
{
    if (!rdram || !g_installed.load(std::memory_order_relaxed))
        return;
    diagnostics(rdram);

    if (active(InfiniteMagic))
    {
        const unsigned level = std::min<unsigned>(rdram[kState + 0x77u], 5u);
        const float max = read<float>(rdram, kSettings + 0x14u + 4u * level);
        if (std::isfinite(max) && max > 0.0f && read<float>(rdram, kState + 0x5Cu) < max)
            write<float>(rdram, kState + 0x5Cu, max);
    }
    if (active(InfiniteRage) && read<float>(rdram, kState + 0x60u) < 100.0f)
        write<float>(rdram, kState + 0x60u, 100.0f);
    if (active(MaxOrbs) && read<uint16_t>(rdram, kState + 0x68u) != 60000u)
        write<uint16_t>(rdram, kState + 0x68u, 60000u);
    if (active(AllUpgrades))
    {
        for (uint32_t i = 0x78u; i <= 0x7Cu; ++i)
            rdram[kState + i] = std::max<uint8_t>(rdram[kState + i], 2u);
        rdram[kState + 0x7Du] = std::max<uint8_t>(rdram[kState + 0x7Du], 4u);
    }
    if (active(AllPowers))
        write<uint16_t>(rdram, kState + 0x82u, static_cast<uint16_t>(read<uint16_t>(rdram, kState + 0x82u) | 0x017Cu));

    const uint32_t player = kratos(rdram);
    const bool playerSafe = player != 0u && kratosPlausible(rdram, player);
    if (active(MaxMeters))
    {
        rdram[kState + 0x76u] = std::max<uint8_t>(rdram[kState + 0x76u], 4u);
        rdram[kState + 0x77u] = std::max<uint8_t>(rdram[kState + 0x77u], 5u);
        if (playerSafe)
        {
            const float max = maxHealthOfLevel(rdram, 4u);
            if (read<float>(rdram, player + 0x17Cu) < max)
                write<float>(rdram, player + 0x17Cu, max);
        }
    }
    if (active(Immortal) && playerSafe)
    {
        // Only refills if still alive: doesn't resurrect mid-death.
        const float health = read<float>(rdram, player + 0x178u), max = read<float>(rdram, player + 0x17Cu);
        if (health > 0.0f && health < max)
            write<float>(rdram, player + 0x178u, max);
    }
    if (active(Extras))
    {
        const uint32_t completed = read<uint32_t>(rdram, kCompletedPlaythroughs);
        if ((completed & 0x1Fu) != 0x1Fu)
            write<uint32_t>(rdram, kCompletedPlaythroughs, completed | 0x1Fu);
        const uint32_t variables = read<uint32_t>(rdram, kUiVariables) & kMask;
        if (validAddress(variables, 0x1B4u))
        {
            write<uint32_t>(rdram, variables + 0x1ACu, 3u);
            write<uint32_t>(rdram, variables + 0x1B0u, 1u);
        }
    }
}

// Pad wrapper (EE thread): with the menu open the game receives nothing. After closing it, it still receives
// nothing until the key that closed it is released (Esc is circle with the classic keyboard scheme).
bool gowCheatsInputBlocked()
{
    if (g_menuOpen.load(std::memory_order_acquire))
        return true;
    return g_blockAfterClose.load(std::memory_order_acquire);
}

// Main thread, before drawing: F2 opens and closes; with the menu open, keyboard, mouse and pad drive it.
void gowCheatsMenuInput(bool escClosesGame)
{
    if (!g_installed.load(std::memory_order_relaxed))
        return;
    if (g_blockAfterClose.load(std::memory_order_relaxed) && (g_menu.closeKey == 0 || !IsKeyDown(g_menu.closeKey)) &&
        !IsGamepadButtonDown(0, GAMEPAD_BUTTON_RIGHT_FACE_RIGHT) && !cheatTriggerDown())
        g_blockAfterClose.store(false, std::memory_order_release);

    // Testing (captures without a keyboard): PC_CHEATS_OPEN_IN=<seconds> opens the menu at that time.
    static double openAt = []
    {
        const char *value = std::getenv("PC_CHEATS_OPEN_IN");
        return value ? std::atof(value) : -1.0;
    }();
    const bool customTrigger = cheatTriggerPressed();
    if (cheatTriggerDown())
        g_trigger.latched = true;
    else
        g_trigger.latched = false;
    const bool isOpen = g_menuOpen.load(std::memory_order_relaxed);
    if (!isOpen)
    {
        const bool test = openAt >= 0.0 && GetTime() >= openAt;
        if (test)
        {
            openAt = -1.0;
            // GOW-Port: PC_CHEATS_OPEN_PAGE=saves opens the test menu on the Saves page
            if (const char *page = std::getenv("PC_CHEATS_OPEN_PAGE"); page && std::strcmp(page, "saves") == 0)
                g_menu.page = 1;
        }
        if (IsKeyPressed(KEY_F2) || test || customTrigger)
            open(escClosesGame);
        return;
    }
    if (IsKeyPressed(KEY_F2) || IsKeyPressed(KEY_ESCAPE) || IsKeyPressed(KEY_BACKSPACE))
    {
        close(escClosesGame, IsKeyPressed(KEY_F2) ? KEY_F2 : IsKeyPressed(KEY_ESCAPE) ? KEY_ESCAPE : KEY_BACKSPACE);
        return;
    }
    if (IsGamepadAvailable(0) && IsGamepadButtonPressed(0, GAMEPAD_BUTTON_RIGHT_FACE_RIGHT))
    {
        close(escClosesGame, 0);
        g_blockAfterClose.store(true, std::memory_order_release);
        return;
    }
    if (customTrigger)
    {
        close(escClosesGame, 0);
        g_blockAfterClose.store(true, std::memory_order_release);
        return;
    }

    const bool gamepad = IsGamepadAvailable(0);
    // GOW-Port: two pages, cheats and saves
    if (IsKeyPressed(KEY_TAB) || IsKeyPressed(KEY_Q) || IsKeyPressed(KEY_E) ||
        (gamepad && (IsGamepadButtonPressed(0, GAMEPAD_BUTTON_LEFT_TRIGGER_1) || IsGamepadButtonPressed(0, GAMEPAD_BUTTON_RIGHT_TRIGGER_1))))
    {
        g_menu.page = 1 - g_menu.page;
        g_menu.armed = -1;
    }
    const int rows = rowCount();
    int &selected = g_menu.page == 0 ? g_menu.selected : g_menu.savesSelected;
    if (rows == 0)
        return;
    selected = std::clamp(selected, 0, rows - 1);
    const int before = selected;
    if (IsKeyPressed(KEY_UP) || IsKeyPressedRepeat(KEY_UP) || IsKeyPressed(KEY_W) ||
        (gamepad && IsGamepadButtonPressed(0, GAMEPAD_BUTTON_LEFT_FACE_UP)))
        selected = (selected + rows - 1) % rows;
    if (IsKeyPressed(KEY_DOWN) || IsKeyPressedRepeat(KEY_DOWN) || IsKeyPressed(KEY_S) ||
        (gamepad && IsGamepadButtonPressed(0, GAMEPAD_BUTTON_LEFT_FACE_DOWN)))
        selected = (selected + 1) % rows;
    if (selected != before)
        g_menu.armed = -1;
    const auto activate = [&](int row) {
        if (g_menu.page == 0)
            toggle(row);
        else
            activateSave(row, escClosesGame);
    };
    if (IsKeyPressed(KEY_ENTER) || IsKeyPressed(KEY_KP_ENTER) || IsKeyPressed(KEY_SPACE) ||
        (g_menu.page == 0 && (IsKeyPressed(KEY_LEFT) || IsKeyPressed(KEY_RIGHT))) ||
        (gamepad && IsGamepadButtonPressed(0, GAMEPAD_BUTTON_RIGHT_FACE_DOWN)))
    {
        activate(selected);
        return;
    }

    const Geometry g = geometry();
    const Vector2 mouse = GetMousePosition();
    const float rowsY = g.y + g.header;
    if (mouse.x >= g.x && mouse.x < g.x + g.w && mouse.y >= rowsY && mouse.y < rowsY + g.row * static_cast<float>(rows))
    {
        const int over = std::clamp(static_cast<int>((mouse.y - rowsY) / g.row), 0, rows - 1);
        const Vector2 delta = GetMouseDelta();
        if ((delta.x != 0.0f || delta.y != 0.0f) && over != selected)
        {
            selected = over;
            g_menu.armed = -1;
        }
        if (IsMouseButtonPressed(MOUSE_BUTTON_LEFT))
        {
            selected = over;
            activate(over);
        }
    }
}

// Main thread, inside BeginDrawing.
void gowCheatsMenuDraw()
{
    if (!g_menuOpen.load(std::memory_order_relaxed))
        return;
    const Fonts &f = fonts();
    const Texts &t = texts();
    const Geometry g = geometry();
    const float s = g.scale;
    const float line = std::max(1.0f, s);

    DrawRectangle(0, 0, GetScreenWidth(), GetScreenHeight(), Color{20, 11, 6, 190});
    const Rectangle panel{g.x, g.y, g.w, g.h};
    // Drop shadow, parchment sheet, bronze rim with a thin inner line.
    DrawRectangleRec(Rectangle{g.x + 6.0f * s, g.y + 8.0f * s, g.w, g.h}, Color{0, 0, 0, 90});
    DrawRectangleGradientV(static_cast<int>(g.x), static_cast<int>(g.y), static_cast<int>(g.w), static_cast<int>(g.h), kParchmentTop,
                           kParchmentBottom);
    DrawRectangleLinesEx(panel, std::max(2.0f, 3.0f * s), kBronze);
    const float inset = 8.0f * s;
    DrawRectangleLinesEx(Rectangle{g.x + inset, g.y + inset, g.w - inset * 2.0f, g.h - inset * 2.0f}, line, kBronzeLight);
    // Corner ornaments: a framed red square at each inner corner.
    for (const float cx : {g.x + inset, g.x + g.w - inset - 16.0f * s})
        for (const float cy : {g.y + inset, g.y + g.h - inset - 16.0f * s})
        {
            DrawRectangleRec(Rectangle{cx, cy, 16.0f * s, 16.0f * s}, kParchmentTop);
            DrawRectangleLinesEx(Rectangle{cx, cy, 16.0f * s, 16.0f * s}, std::max(1.5f, 2.0f * s), kBronze);
            DrawRectangleRec(Rectangle{cx + 5.0f * s, cy + 5.0f * s, 6.0f * s, 6.0f * s}, kRed);
        }

    const float margin = 36.0f * s;
    const float titleSize = 30.0f * s, subSize = 15.0f * s;
    const bool savesPage = g_menu.page == 1;
    drawText(f.bold, savesPage ? t.savesTitle : t.title, g.x + margin, g.y + 24.0f * s, titleSize, kInk);
    drawText(f.regular, savesPage ? t.savesSubtitle : t.subtitle, g.x + margin, g.y + 24.0f * s + titleSize + 6.0f * s, subSize,
             kInkMuted);
    // GOW-Port: page tabs at the right of the header; the current one is red and underlined
    {
        const float tabSize = 17.0f * s, gap = 22.0f * s;
        float tx = g.x + g.w - margin;
        for (int page = 1; page >= 0; --page)
        {
            const char *label = page == 0 ? t.tabCheats : t.tabSaves;
            const float width = textWidth(f.bold, label, tabSize);
            tx -= width;
            const float ty = g.y + 30.0f * s;
            drawText(f.bold, label, tx, ty, tabSize, page == g_menu.page ? kRed : kInkMuted);
            if (page == g_menu.page)
                DrawLineEx(Vector2{tx, ty + tabSize + 4.0f * s}, Vector2{tx + width, ty + tabSize + 4.0f * s}, std::max(1.5f, 2.0f * s), kRed);
            tx -= gap;
        }
    }
    // Header rule with a red gem at its start, as in the launcher's group titles.
    const float ruleY = g.y + g.header - 4.0f * s;
    DrawLineEx(Vector2{g.x + margin, ruleY}, Vector2{g.x + g.w - margin, ruleY}, std::max(1.5f, 2.0f * s), kBronze);
    DrawPoly(Vector2{g.x + margin + 6.0f * s, ruleY}, 4, 8.0f * s, 0.0f, kRed);

    const float nameSize = 21.0f * s, descSize = 15.0f * s;
    const float switchW = 54.0f * s, switchH = 28.0f * s;
    const std::vector<GowSaveSlotInfo> &slots = menuSlots();
    const int rows = savesPage ? static_cast<int>(slots.size()) : kNumCheats;
    const int selectedRow = savesPage ? g_menu.savesSelected : g_menu.selected;
    if (savesPage && rows == 0)
    {
        const std::string empty = truncate(f.regular, t.savesEmpty, descSize + 2.0f * s, g.w - margin * 2.0f);
        drawText(f.regular, empty.c_str(), g.x + margin, g.y + g.header + 24.0f * s, descSize + 2.0f * s, kInkMuted);
    }
    for (int i = 0; i < rows; ++i)
    {
        const float fy = g.y + g.header + g.row * static_cast<float>(i);
        if (fy + g.row > g.y + g.h - g.footer + 1.0f)
            break;
        const bool on = !savesPage && g_active[i].load(std::memory_order_relaxed);
        const bool sel = i == selectedRow;
        const float rowX = g.x + inset + 6.0f * s, rowW = g.w - (inset + 6.0f * s) * 2.0f;
        if (sel)
        {
            // Highlighted row: flat blood red with cream text, as the hovered option of the launcher's dropdowns.
            DrawRectangleGradientV(static_cast<int>(rowX), static_cast<int>(fy + 3.0f * s), static_cast<int>(rowW),
                                   static_cast<int>(g.row - 6.0f * s), kRed, kRedDark);
            DrawRectangleLinesEx(Rectangle{rowX, fy + 3.0f * s, rowW, g.row - 6.0f * s}, line, Color{255, 220, 190, 70});
        }
        const float textX = g.x + margin;
        const float block = nameSize + 6.0f * s + descSize;
        const float ty = fy + (g.row - block) * 0.5f;
        if (savesPage)
        {
            // GOW-Port: a slot as the game's own list shows it: place, then difficulty, play time and slot number
            const GowSaveSlotInfo &slot = slots[i];
            const bool armed = g_menu.armed == i && GetTime() - g_menu.armedAt <= 4.0;
            const char *action = armed ? t.confirm : t.load;
            const float actionSize = 15.0f * s;
            const float actionW = textWidth(f.bold, action, actionSize) + 28.0f * s, actionH = 30.0f * s;
            const float textMax = g.w - margin * 2.0f - actionW - 20.0f * s;
            std::string name = slot.place.empty() ? std::string(t.slot) + " " + std::to_string(slot.slot + 1) : slot.place;
            name = truncate(f.bold, name.c_str(), nameSize, textMax);
            drawText(f.bold, name.c_str(), textX, ty, nameSize, sel ? kCream : kInk);
            std::string detail = armed ? std::string(t.confirmText)
                                       : (slot.difficultyName.empty() ? std::string() : slot.difficultyName + "  ·  ") +
                                             gowSavePlayTime(slot.playSeconds) + "  ·  " + t.slot + " " + std::to_string(slot.slot + 1);
            detail = truncate(f.regular, detail.c_str(), descSize, textMax);
            drawText(f.regular, detail.c_str(), textX, ty + nameSize + 6.0f * s, descSize,
                     sel ? Color{243, 228, 195, 225} : (armed ? kRed : kInkMuted));
            const Rectangle button{g.x + g.w - margin - actionW, fy + (g.row - actionH) * 0.5f, actionW, actionH};
            DrawRectangleRounded(button, 0.35f, 8, armed ? Color{140, 31, 20, 255} : (sel ? kCream : Color{185, 165, 125, 255}));
            DrawRectangleRoundedLinesEx(button, 0.35f, 8, std::max(1.5f, 2.0f * s), armed ? Color{90, 15, 8, 255} : kBronze);
            drawText(f.bold, action, button.x + 14.0f * s, button.y + (actionH - actionSize) * 0.5f, actionSize, armed ? kCream : kInk);
        }
        else
        {
            const float textMax = g.w - margin * 2.0f - switchW - 20.0f * s;
            const Color nameColor = sel ? kCream : (on ? kRed : kInk);
            drawText(f.bold, t.name[i], textX, ty, nameSize, nameColor);
            const std::string desc = truncate(f.regular, t.description[i], descSize, textMax);
            drawText(f.regular, desc.c_str(), textX, ty + nameSize + 6.0f * s, descSize, sel ? Color{243, 228, 195, 225} : kInkMuted);

            // Switch: red track with a cream knob when on, tan track when off.
            const Rectangle track{g.x + g.w - margin - switchW, fy + (g.row - switchH) * 0.5f, switchW, switchH};
            DrawRectangleRounded(track, 1.0f, 12, on ? Color{140, 31, 20, 255} : Color{185, 165, 125, 255});
            DrawRectangleRoundedLinesEx(track, 1.0f, 12, std::max(1.5f, 2.0f * s), on ? Color{90, 15, 8, 255} : kBronze);
            const float radius = switchH * 0.5f - 3.5f * s;
            const float cx = on ? track.x + track.width - switchH * 0.5f : track.x + switchH * 0.5f;
            DrawCircleV(Vector2{cx, track.y + switchH * 0.5f}, radius, kCream);
            DrawCircleLines(static_cast<int>(cx), static_cast<int>(track.y + switchH * 0.5f), radius, kBronze);
        }

        if (i + 1 < rows && !sel && i + 1 != selectedRow)
            DrawLineEx(Vector2{g.x + margin, fy + g.row}, Vector2{g.x + g.w - margin, fy + g.row}, 1.0f, Color{95, 67, 38, 70});
    }

    const float footerY = g.y + g.h - g.footer;
    DrawLineEx(Vector2{g.x + margin, footerY}, Vector2{g.x + g.w - margin, footerY}, std::max(1.5f, 2.0f * s), kBronze);
    const float footerTextSize = 14.0f * s;
    const float footerMaxWidth = g.w - margin * 2.0f;
    const std::string navigate = std::string(savesPage ? t.savesHelp : t.navigateHelp) + "    " + t.pageHelp;
    const std::string navigateLine = truncate(f.regular, navigate.c_str(), footerTextSize, footerMaxWidth);
    drawText(f.regular, navigateLine.c_str(), g.x + margin, footerY + 11.0f * s, footerTextSize, kInkSoft);
    std::string closeHelp = t.closeHelp;
    if (const std::string shortcut = triggerHelp(); !shortcut.empty())
        closeHelp += "    " + shortcut + " " + t.shortcutHelp;
    closeHelp = truncate(f.regular, closeHelp.c_str(), footerTextSize, footerMaxWidth);
    drawText(f.regular, closeHelp.c_str(), g.x + margin, footerY + 36.0f * s, footerTextSize, kInkSoft);
}
