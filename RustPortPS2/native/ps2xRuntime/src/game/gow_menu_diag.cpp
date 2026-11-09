// GOW-Port: bounded, read-only diagnostics for the original UI of God of War (SCUS-97399). See
// docs/dev/NATIVE_MENU_RESEARCH.md for what each address is and how it was established.
//
// The retail menus are Flash-style movies driven by a C++ "HUD object" (*0x29CB94). The movie reads and writes the
// object's named variables as `PS2_<name>`; text for prompts comes from the `msgs_en.txt` bank. These hooks only observe
// that machinery and always call the generated original (including when entered at a checkpoint resumption), so they
// cannot change behavior. They are inert unless GOW_MENU_DIAG=1 or GOW_NATIVE_CHEATS_MENU=1 is set.
//
//   0x1B8A48  LoadMessages(): reads msgs_en.txt, builds the string table (*0x29CBA4) and the id index (*0x29CBA0)
//   0x1B9358  ShowPrompt(hud, id): fills MenuTemplate_Line1..3 / Menu1..5 from the bank block `id` (Yes/No style prompts)
//   0x1B7CE0  LoadMovie(hud, name): binds a Flash movie (slot hud+0x1558+4*n) and exposes the HUD variables to it

#include "gow_game.h"
#include "gow_guest_functions.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <utility>
#include <vector>

#include "../lib/gow_cheats.h"
#include "../lib/gow_image.h"
#include "../lib/gow_frame.h"
#include "../lib/gow_quality.h"
#include "../lib/gow_overlay.h"
#include "gow_menu_patch.h"
#include "gow_menu_patch_shell.h"

namespace gow_game
{
    namespace
    {
        constexpr uint32_t kHudPointer = 0x0029CB94u;     // *kHudPointer = HUD object
        constexpr uint32_t kBankIndex = 0x0029CBA0u;      // *: array of {u16 id, u16 first string}
        constexpr uint32_t kBankStrings = 0x0029CBA4u;    // *: array of char* (one per line)
        constexpr uint32_t kBankIndexCount = 0x0029CBA8u; // number of index entries
        constexpr uint32_t kBankStringCount = 0x0029CBACu;
        constexpr uint32_t kVariableTable = 0x2BCu;       // HUD object: table of named variables (0x18 bytes each)
        constexpr uint32_t kVariableCapacity = 0xBFu;
        constexpr uint32_t kHudSizeNeeded = kVariableTable + kVariableCapacity * 0x18u;

        bool diagEnabled()
        {
            static const bool value = [] {
                const char *a = std::getenv("GOW_MENU_DIAG");
                const char *b = std::getenv("GOW_NATIVE_CHEATS_MENU");
                return (a && a[0] == '1') || (b && b[0] == '1');
            }();
            return value;
        }

        // Bounded copy of a guest C string; returns false when the pointer is outside RAM.
        bool readString(uint8_t *rdram, uint32_t address, char *out, size_t capacity)
        {
            const uint8_t *src = guestBuffer(rdram, address, 1u);
            if (!src || capacity == 0u)
                return false;
            const uint32_t physical = address & 0x1fffffffu;
            const size_t room = 0x02000000u - physical;
            size_t n = 0;
            while (n + 1 < capacity && n < room && src[n] != 0u)
            {
                const unsigned char c = src[n];
                out[n] = (c >= 32u && c < 127u) ? static_cast<char>(c) : '?';
                ++n;
            }
            out[n] = 0;
            return true;
        }

        // Looks up a registered HUD variable by name. Returns the address of its storage inside the HUD object, or 0.
        uint32_t variableAddress(uint8_t *rdram, uint32_t hud, const char *name, uint32_t *typeOut = nullptr)
        {
            for (uint32_t i = 0; i < kVariableCapacity; ++i)
            {
                const uint32_t entry = hud + kVariableTable + i * 0x18u;
                const uint32_t namePtr = readGuest32(rdram, entry);
                if (namePtr == 0u)
                    continue;
                char text[48];
                if (!readString(rdram, namePtr, text, sizeof(text)) || std::strcmp(text, name) != 0)
                    continue;
                if (typeOut)
                    *typeOut = readGuest32(rdram, entry + 4u);
                return readGuest32(rdram, entry + 8u);
            }
            return 0u;
        }

        // Checks the structure the research recovered (the variable table of the HUD object). Logged once. This is the
        // check any code that touches the table must pass first, so that an unexpected layout (another game version,
        // a modified ELF) disables the extension instead of writing to unknown memory.
        bool validateHud(uint8_t *rdram)
        {
            static bool done = false, result = false;
            if (done)
                return result;
            const uint32_t hud = readGuest32(rdram, kHudPointer);
            if (!guestBuffer(rdram, hud, kHudSizeNeeded))
                return false; // not created yet; try again on the next call
            done = true;
            unsigned used = 0;
            for (uint32_t i = 0; i < kVariableCapacity; ++i)
                if (readGuest32(rdram, hud + kVariableTable + i * 0x18u) != 0u)
                    ++used;
            struct Expect { const char *name; uint32_t offset; };
            static const Expect expect[] = {{"MenuTemplate_Menu1", 0xACu}, {"MenuTemplate_Event", 0x208u},
                                            {"PauseMenu_Event", 0x254u},   {"Menu_Choice", 0x278u},
                                            {"Vibration", 0x298u},         {"WideScreen", 0x29Cu},
                                            {"ProgressiveScan", 0x2A0u},   {"SoundMode", 0x2A8u}};
            unsigned ok = 0;
            for (const Expect &e : expect)
            {
                const uint32_t address = variableAddress(rdram, hud, e.name);
                if (address == hud + e.offset)
                    ++ok;
                else
                    std::fprintf(stderr, "[gow-menu] variable %s: expected %08x, found %08x\n", e.name, hud + e.offset, address);
            }
            std::fprintf(stderr, "[gow-menu] HUD object %08x: %u variables registered, %u/%zu layout checks passed\n", hud,
                         used, ok, sizeof(expect) / sizeof(expect[0]));
            result = ok == sizeof(expect) / sizeof(expect[0]) && used < kVariableCapacity;
            return result;
        }
    }

    // ---- Port Options extension (GOW_NATIVE_CHEATS_MENU=1) -------------------------------------------------------------
    // The pause menu is movie content, so the extension is a movie with one more row ("Port Options", below Options), a list
    // page with "Cheats", and the Cheats page, handed to the game when it builds the HUD movie. The movie reads and writes one HUD
    // variable per cheat (PS2_<name>, registered here, storage = a guest word) and a VBlank callback keeps each equal to the
    // cheat engine's state.
    namespace
    {
        // The movies reach the settings through ten HUD variables (the table of the game's HUD object has 191 entries and the game uses
        // 170): PS2_PortPage tells which page is open, PS2_Slot0..PS2_Slot8 hold that page's values, row by row. The page is armed
        // while its row of the Port Options list is highlighted (the movie sets PortPage as the highlight moves), so the slots
        // already hold the right values when the page opens; a VBlank callback copies the page's current settings into the slots and
        // a slot the movie changed into its setting. The page numbers and rows are the ones of tools/menu/flp_patch_cheats.py
        // (PAGE_ID, PAGES).
        constexpr int kSlotCount = 9;
        constexpr int kVarCount = 1 + kSlotCount;   // word 0: PortPage, words 1..9: the slots
        constexpr const char *kVarNames[kVarCount] = {"PortPage", "Slot0", "Slot1", "Slot2", "Slot3",
                                                      "Slot4",    "Slot5", "Slot6", "Slot7", "Slot8"};
        constexpr uint32_t kVarBlockSize = 256u;    // 4 bytes per variable (the values), then 16 bytes per name from offset 96
        constexpr uint32_t kNameOffset = 96u;
        static_assert(kNameOffset + 16u * kVarCount <= kVarBlockSize && 4u * kVarCount <= kNameOffset, "variable block layout");
        enum Page { PageNone = 0, PageCheats, PageOverlay, PageImage, PageQuality, PageFrame, PageCount };
        constexpr int kPageSlots[PageCount] = {0, 9, 4, 4, 4, 3};
        constexpr const char *kCheatNames[9] = {"GodMode", "OneHitKill", "InfiniteMagic", "InfiniteRage", "MaxOrbs",
                                                "AllUpgrades", "AllPowers", "MaxMeters", "UnlockExtras"};
        constexpr const char *kOverlayNames[4] = {"OsdFps", "OsdGpu", "OsdCpu", "OsdRam"};
        constexpr const char *kImageNames[4] = {"ImgBrightness", "ImgContrast", "ImgGamma", "ImgSaturation"};
        constexpr const char *kQualityNames[4] = {"RenderResolution", "Upscaling", "Sharpness", "FXAA"};
        constexpr const char *kFrameNames[3] = {"FpsLimit", "VSync", "EarlyFinish"};

        const char *settingName(int page, int i)
        {
            switch (page)
            {
            case PageCheats: return kCheatNames[i];
            case PageOverlay: return kOverlayNames[i];
            case PageImage: return kImageNames[i];
            case PageQuality: return kQualityNames[i];
            case PageFrame: return kFrameNames[i];
            default: return "?";
            }
        }

        // The largest value of a setting: toggles 1, sliders 100, the FPS limit 2, the render resolution 7, the filter 2.
        uint32_t settingMax(int page, int i)
        {
            if (page == PageImage)
                return 100u;
            if (page == PageQuality)
                return i == 0 ? 7u : (i == 1 ? 2u : (i == 2 ? 100u : 1u));
            if (page == PageFrame)
                return i == 0 ? 2u : 1u;
            return 1u;
        }

        uint32_t settingGet(int page, int i)
        {
            switch (page)
            {
            case PageCheats: return gowCheatsGet(i) ? 1u : 0u;
            case PageOverlay: return gowOverlayGet(i) ? 1u : 0u;
            case PageImage: return static_cast<uint32_t>(std::clamp(gowImageGet(i), 0, 100));
            case PageQuality: return static_cast<uint32_t>(std::max(0, gowQualityGet(i)));
            case PageFrame: return static_cast<uint32_t>(std::max(0, gowFrameGet(i)));
            default: return 0u;
            }
        }

        // false when the setting refuses (the cheats: unavailable, Hardcore)
        bool settingSet(int page, int i, uint32_t value)
        {
            value = std::min(value, settingMax(page, i));
            switch (page)
            {
            case PageCheats: return gowCheatsSet(i, value != 0u);
            case PageOverlay: gowOverlaySet(i, value != 0u); return true;
            case PageImage: gowImageSet(i, static_cast<int>(value)); return true;
            case PageQuality: gowQualitySet(i, static_cast<int>(value)); return true;
            case PageFrame: gowFrameSet(i, static_cast<int>(value)); return true;
            default: return false;
            }
        }

        void logSettingChange(int page, int i, uint32_t value)
        {
            const char *name = settingName(page, i);
            if (page == PageImage || (page == PageQuality && i == 2))
                std::fprintf(stderr, "[gow-menu] %s changed from the Port Options menu: %u%%\n", name, value);
            else if (page == PageFrame && i == 0)
                std::fprintf(stderr, "[gow-menu] %s changed from the Port Options menu: %u FPS\n", name, value == 0u ? 60u : (value == 1u ? 120u : 240u));
            else if (page == PageQuality && i == 0)
                std::fprintf(stderr, "[gow-menu] %s changed from the Port Options menu: %ux\n", name, value + 1u);
            else if (page == PageQuality && i == 1)
                std::fprintf(stderr, "[gow-menu] %s changed from the Port Options menu: %s\n", name,
                             value == 0u ? "bicubic" : (value == 1u ? "bilinear" : "pixel"));
            else
                std::fprintf(stderr, "[gow-menu] %s changed from the Port Options menu: %s\n", name, value ? "ON" : "OFF");
        }
        constexpr uint32_t kHudMovieSize = gow_menu_patch::kOriginalSize; // retail FLP_HUD
        constexpr uint32_t kHudT9Count = 1496u;     // its T9 table (color multipliers)

        struct ExtensionState
        {
            bool attempted = false, ready = false;   // variable registered, VBlank sync active
            bool movieTried = false, movieOk = false; // cheats available, so the movie is patched when the WAD is built
            uint32_t movieData = 0u;                  // guest address of the movie buffer
            uint32_t t9Block = 0u, t8Block = 0u;      // blocks holding the movie's T9 and T8 tables (read-only, left out of the buffer)
            uint32_t varBlock = 0u;                   // storage and names of the HUD variables
            bool tablesDone = false;
            uint32_t last[kSlotCount] = {};           // the slot values as the callback left them
            uint32_t lastPage = 0u;
        };
        ExtensionState g_state;

        bool extensionRequested()
        {
            static const bool value = [] {
                const char *v = std::getenv("GOW_NATIVE_CHEATS_MENU");
                return v && v[0] == '1';
            }();
            return value;
        }

        // Calls a recompiled guest function with register arguments a0..a3, t0, t1; the caller's registers are restored.
        uint32_t callGuest(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime,
                           void (*fn)(uint8_t *, R5900Context *, PS2Runtime *), uint32_t entry, uint32_t a0, uint32_t a1 = 0,
                           uint32_t a2 = 0, uint32_t a3 = 0, uint32_t t0 = 0, uint32_t t1 = 0)
        {
            const R5900Context saved = *ctx;
            SET_GPR_U32(ctx, 4, a0);
            SET_GPR_U32(ctx, 5, a1);
            SET_GPR_U32(ctx, 6, a2);
            SET_GPR_U32(ctx, 7, a3);
            SET_GPR_U32(ctx, 8, t0);
            SET_GPR_U32(ctx, 9, t1);
            ctx->pc = entry;
            fn(rdram, ctx, runtime);
            const uint32_t result = GPR_U32(ctx, 2);
            *ctx = saved;
            return result;
        }

        using Ranges = std::vector<std::pair<uint32_t, uint32_t>>;

        // CRC32 of `data` where the bytes inside the (sorted, disjoint) `masked` ranges count as zero.
        uint32_t crc32(const uint8_t *data, size_t size, const Ranges &masked = Ranges())
        {
            static uint32_t table[256];
            static bool ready = false;
            if (!ready)
            {
                for (uint32_t i = 0; i < 256u; ++i)
                {
                    uint32_t c = i;
                    for (int k = 0; k < 8; ++k)
                        c = (c & 1u) ? 0xEDB88320u ^ (c >> 1) : c >> 1;
                    table[i] = c;
                }
                ready = true;
            }
            uint32_t c = 0xFFFFFFFFu;
            size_t next = 0;
            for (size_t i = 0; i < size; ++i)
            {
                while (next < masked.size() && masked[next].second <= i)
                    ++next;
                const bool zero = next < masked.size() && i >= masked[next].first;
                c = table[(c ^ (zero ? 0u : data[i])) & 0xFFu] ^ (c >> 8);
            }
            return ~c;
        }

        // One embedded delta: the retail movie it applies to and the movie it makes (gow_menu_patch.h: the pause menu, FLP_HUD;
        // gow_menu_patch_shell.h: the title menu, FLP_Shell). A volatile window (begin < end) is a part of the loaded movie that the game
        // itself writes: checksums leave it out and the copies over it keep whatever the game has there.
        struct PatchInfo
        {
            uint32_t originalSize, originalCrc, patchedSize, patchedCrc, volatileBegin, volatileEnd;
            const uint8_t *ops;
            size_t opsSize;
        };
        const PatchInfo kHudPatch{gow_menu_patch::kOriginalSize, gow_menu_patch::kOriginalCrc32, gow_menu_patch::kPatchedSize,
                                  gow_menu_patch::kPatchedCrc32, gow_menu_patch::kVolatileBegin, gow_menu_patch::kVolatileEnd,
                                  gow_menu_patch::kOps, sizeof(gow_menu_patch::kOps)};
        const PatchInfo kShellPatch{gow_menu_patch_shell::kOriginalSize, gow_menu_patch_shell::kOriginalCrc32, gow_menu_patch_shell::kPatchedSize,
                                    gow_menu_patch_shell::kPatchedCrc32, 0u, 0u, gow_menu_patch_shell::kOps, sizeof(gow_menu_patch_shell::kOps)};

        // Applies the embedded delta `info` to the retail movie `original` (info.originalSize bytes) and checks the result against its
        // size and CRC32. The delta carries the new records only; everything else comes from the movie itself, so the repository
        // holds nothing of the game.
        bool applyPatch(const PatchInfo &info, const uint8_t *original, std::vector<uint8_t> &out)
        {
            const Ranges window = info.volatileBegin < info.volatileEnd ? Ranges{{info.volatileBegin, info.volatileEnd}} : Ranges();
            if (crc32(original, info.originalSize, window) != info.originalCrc)
                return false;
            Ranges masked; // output ranges copied from the window: whatever the game has there stays (as in the original menu)
            out.clear();
            out.reserve(info.patchedSize);
            const uint8_t *p = info.ops, *end = info.ops + info.opsSize;
            auto varint = [&](uint32_t &v) {
                v = 0;
                for (unsigned shift = 0; p < end && shift < 32u; shift += 7u)
                {
                    const uint8_t c = *p++;
                    v |= static_cast<uint32_t>(c & 0x7Fu) << shift;
                    if (!(c & 0x80u))
                        return true;
                }
                return false;
            };
            while (p < end)
            {
                const uint8_t op = *p++;
                uint32_t a = 0, n = 0;
                if (op == 2)
                    return out.size() == info.patchedSize && crc32(out.data(), out.size(), masked) == info.patchedCrc;
                if (op == 0)
                {
                    if (!varint(n) || n > static_cast<uint32_t>(end - p))
                        return false;
                    out.insert(out.end(), p, p + n);
                    p += n;
                }
                else if (op == 1)
                {
                    if (!varint(a) || !varint(n) || a > info.originalSize || n > info.originalSize - a)
                        return false;
                    const uint32_t lo = a > info.volatileBegin ? a : info.volatileBegin, hi = a + n < info.volatileEnd ? a + n : info.volatileEnd;
                    if (lo < hi)
                    {
                        const uint32_t at = static_cast<uint32_t>(out.size());
                        masked.emplace_back(at + (lo - a), at + (hi - a));
                    }
                    out.insert(out.end(), original + a, original + a + n);
                }
                else
                    return false;
                if (out.size() > info.patchedSize)
                    return false;
            }
            return false;
        }

        // The movie is built when R_PERM.WAD is loaded, long before the HUD object exists, so the decision is taken on the
        // first BuildMovie call: cheats must be available (not Hardcore, not a foreign game).
        bool ensureMovie()
        {
            if (g_state.movieTried)
                return g_state.movieOk;
            g_state.movieTried = true;
            if (!gowCheatsAvailable())
            {
                std::fprintf(stderr, "[gow-menu] extension off: cheats are unavailable (not God of War, Hardcore, or missing hooks)\n");
                return false;
            }
            g_state.movieOk = true;
            return true;
        }

        // Gives the patched movie its T8 and T9 tables once the loader has walked the whole buffer (T10, the last table, then has a
        // pointer inside the buffer). Called from Hud_LoadMovie, long after the WAD was built.
        void finishMovie(uint8_t *rdram)
        {
            if (g_state.tablesDone || !g_state.movieData || !g_state.t9Block || !g_state.t8Block)
                return;
            const uint32_t data = g_state.movieData;
            const uint32_t pool = readGuest32(rdram, data + 0x5Cu);
            if (pool < data || pool >= data + kHudMovieSize)
                return; // not walked yet (or never will be): try again on the next call
            g_state.tablesDone = true;
            auto setTable = [&](uint32_t countAt, uint32_t block, uint32_t count) {
                writeGuest32(rdram, data + countAt + 4u, block); // the pointer follows the 16-bit count
                if (uint8_t *at = guestBuffer(rdram, data + countAt, 2))
                {
                    const uint16_t n = static_cast<uint16_t>(count);
                    std::memcpy(at, &n, 2);
                }
            };
            setTable(0x48u, g_state.t8Block, gow_menu_patch::kT8Size / 20u);
            setTable(0x50u, g_state.t9Block, kHudT9Count);
            std::fprintf(stderr, "[gow-menu] patched movie complete: T8 (%u entries) at %08x, T9 (%u entries) at %08x\n",
                         gow_menu_patch::kT8Size / 20u, g_state.t8Block, kHudT9Count, g_state.t9Block);
            if (std::getenv("GOW_MENU_WINDOW_DUMP"))
                if (const uint8_t *w = guestBuffer(rdram, data + gow_menu_patch::kVolatileBegin - 0x10u, 0xE0u))
                {
                    std::fprintf(stderr, "[gow-menu] window %05x:", gow_menu_patch::kVolatileBegin - 0x10u);
                    for (uint32_t i = 0; i < 0xE0u; ++i)
                        std::fprintf(stderr, "%s%02x", i % 16u == 0 ? "\n   " : " ", w[i]);
                    std::fprintf(stderr, "\n");
                }
        }

        // First Hud_LoadMovie: the HUD object exists. Register the variables the movie reads (PS2_<cheat>).
        void prepareExtension(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime, uint32_t hud)
        {
            g_state.attempted = true;
            if (!g_state.movieOk || !g_state.varBlock) // no variable block: the patch was not applied, the stock menu is running
                return;
            if (!validateHud(rdram))
            {
                std::fprintf(stderr, "[gow-menu] extension off: HUD object layout not recognized\n");
                return;
            }
            for (int i = 0; i < kVarCount; ++i)
            {
                uint8_t *name = guestBuffer(rdram, g_state.varBlock + kNameOffset + 16u * i, 16);
                uint8_t *word = guestBuffer(rdram, g_state.varBlock + 4u * i, 4);
                if (!name || !word)
                    return;
                std::memset(name, 0, 16);
                std::memcpy(name, kVarNames[i], std::strlen(kVarNames[i]));
                const uint32_t initial = 0u;
                std::memcpy(word, &initial, 4);
                // Hud_RegisterVariable(hud, type 7, name, flag, storage, default 0): the movies see it as PS2_<name>.
                // flag 0 (a3): the movie may write it (Hud_SetVariable ignores writes to entries whose +0xC is not zero), like Vibration
                callGuest(rdram, ctx, runtime, sub_001B5CE0_0x1b5ce0, 0x001B5CE0u, hud, 7u, g_state.varBlock + kNameOffset + 16u * i, 0u,
                          g_state.varBlock + 4u * i, 0u);
            }
            g_state.ready = true;
            std::fprintf(stderr, "[gow-menu] Port Options pages: %d variables PS2_<name> registered (PortPage and %d slots)\n", kVarCount,
                         kSlotCount);
        }
    }

    // 0x14AE68 BuildMovie(a0, a1 = resource {+2 flags, +4 size}, a2 = data): when the data is the retail FLP_HUD (which the
    // loader parses in place, flags bit 0x2000), the patched movie is written over it and the loader walks that instead.
    // The patched movie is smaller than the original because its two read-only tables, T8 (20-byte transforms) and T9 (8-byte
    // color multipliers), are left out: they live in blocks of their own and their counts and pointers (movie header +0x48/+0x4C and
    // +0x50/+0x54) are set after the parse. The memory budget of the game does not allow keeping a second copy of the 480 KB movie,
    // while a few hundred KB of extra heap are fine.
    void gowNativeMenuBuildMovie(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x0014AE68u || !ensureMovie())
        {
            sub_0014AE68_0x14ae68(rdram, ctx, runtime);
            return;
        }
        const uint32_t resource = GPR_U32(ctx, 5), data = GPR_U32(ctx, 6);
        if (const char *dump = std::getenv("GOW_MENU_DUMP_MOVIE"); dump && diagEnabled())
        {
            // research aid: one line per movie built; GOW_MENU_DUMP_MOVIE=<size> also writes the buffer of the movie of that size
            const uint32_t size = readGuest32(rdram, resource + 4u), flags = readGuest32(rdram, resource) >> 16;
            std::fprintf(stderr, "[gow-menu] BuildMovie resource=%08x data=%08x size=%u flags=%04x t0=%u t3=%u t6=%u\n", resource, data, size,
                         flags, readGuest32(rdram, data + 0x0Cu), readGuest32(rdram, data + 0x24u), readGuest32(rdram, data + 0x3Cu));
            if (size == static_cast<uint32_t>(std::atol(dump)))
                if (const uint8_t *bytes = guestBuffer(rdram, data, size))
                    if (std::FILE *f = std::fopen("movie_dump.bin", "wb"))
                    {
                        std::fwrite(bytes, 1, size, f);
                        std::fclose(f);
                    }
        }
        const uint8_t *res = guestBuffer(rdram, resource, 8);
        const uint8_t *src = guestBuffer(rdram, data, kHudMovieSize);
        const bool inPlace = res && ((res[2] | (res[3] << 8)) & 0x2000) != 0;
        if (res && inPlace && g_state.varBlock && readGuest32(rdram, resource + 4u) == gow_menu_patch_shell::kOriginalSize &&
            readGuest32(rdram, data + 0x0Cu) == 373u && readGuest32(rdram, data + 0x24u) == 113u && readGuest32(rdram, data + 0x3Cu) == 135u)
        {
            // FLP_Shell, the title menu: the same delta mechanism, but the patched movie (tables included) is bigger than the buffer the
            // game loaded the retail one into, so the loader parses it in place in a block of its own and the original buffer stays
            // unused. Only after the pause menu patch took effect: the pages use the variables it registers.
            const uint8_t *shellSrc = guestBuffer(rdram, data, gow_menu_patch_shell::kOriginalSize);
            std::vector<uint8_t> shellPatched;
            if (!shellSrc || !applyPatch(kShellPatch, shellSrc, shellPatched))
            {
                std::fprintf(stderr, "[gow-menu] FLP_Shell is not the retail movie the title menu patch was made for; original title menu kept\n");
                sub_0014AE68_0x14ae68(rdram, ctx, runtime);
                return;
            }
            // The block is bigger than the movie and cleared: the loader writes a little past the end of the data it walks, and the
            // title menu came out black (or the game stopped) for some patched sizes when the block was exactly the movie's size.
            const uint32_t blockSize = static_cast<uint32_t>(shellPatched.size()) + 0x2000u;
            const uint32_t heap = callGuest(rdram, ctx, runtime, sub_0013E090_0x13e090, 0x0013E090u, 0u);
            const uint32_t block = callGuest(rdram, ctx, runtime, sub_0013DC78_0x13dc78, 0x0013DC78u, heap, blockSize, 0x10u);
            uint8_t *dst = guestBuffer(rdram, block, blockSize);
            if (block && dst)
                std::memset(dst, 0, blockSize);
            if (!block || !dst)
            {
                std::fprintf(stderr, "[gow-menu] could not allocate %zu bytes for the title menu movie; original title menu kept\n",
                             shellPatched.size());
                sub_0014AE68_0x14ae68(rdram, ctx, runtime);
                return;
            }
            std::memcpy(dst, shellPatched.data(), shellPatched.size());
            std::fprintf(stderr, "[gow-menu] FLP_Shell replaced by the patched title menu (%u -> %zu bytes, block at %08x)\n",
                         gow_menu_patch_shell::kOriginalSize, shellPatched.size(), block);
            SET_GPR_U32(ctx, 6, block);
            sub_0014AE68_0x14ae68(rdram, ctx, runtime);
            return;
        }
        const bool match = res && src && inPlace && readGuest32(rdram, resource + 4u) == kHudMovieSize &&
                           readGuest32(rdram, data + 0x0Cu) == 936u && readGuest32(rdram, data + 0x14u) == 220u &&
                           readGuest32(rdram, data + 0x24u) == 199u && readGuest32(rdram, data + 0x3Cu) == 381u &&
                           (readGuest32(rdram, data + 0x50u) & 0xFFFFu) == kHudT9Count;
        if (!match)
        {
            sub_0014AE68_0x14ae68(rdram, ctx, runtime);
            return;
        }
        std::vector<uint8_t> patched;
        if (!applyPatch(kHudPatch, src, patched))
        {
            std::fprintf(stderr, "[gow-menu] FLP_HUD is not the retail movie the CHEATS patch was made for (CRC32 %08x); original menu kept\n",
                         crc32(src, kHudMovieSize, Ranges{{gow_menu_patch::kVolatileBegin, gow_menu_patch::kVolatileEnd}}));
            sub_0014AE68_0x14ae68(rdram, ctx, runtime);
            return;
        }
        const uint32_t t9Size = gow_menu_patch::kT9Size, t8Size = gow_menu_patch::kT8Size;
        const uint32_t heap = callGuest(rdram, ctx, runtime, sub_0013E090_0x13e090, 0x0013E090u, 0u);
        auto take = [&](uint32_t size) { return callGuest(rdram, ctx, runtime, sub_0013DC78_0x13dc78, 0x0013DC78u, heap, size, 0x10u); };
        const uint32_t t9Block = take(t9Size), t8Block = take(t8Size), varBlock = take(kVarBlockSize);
        uint8_t *t9Dst = guestBuffer(rdram, t9Block, t9Size), *t8Dst = guestBuffer(rdram, t8Block, t8Size);
        uint8_t *varDst = guestBuffer(rdram, varBlock, kVarBlockSize);
        if (!t9Block || !t8Block || !varBlock || !t9Dst || !t8Dst || !varDst)
        {
            std::fprintf(stderr, "[gow-menu] could not allocate the extra blocks (T9 %08x, T8 %08x, variables %08x); original menu kept\n",
                         t9Block, t8Block, varBlock);
            sub_0014AE68_0x14ae68(rdram, ctx, runtime);
            return;
        }
        std::memcpy(t9Dst, src + gow_menu_patch::kT9Offset, t9Size); // before the movie is overwritten
        std::memcpy(t8Dst, patched.data() + gow_menu_patch::kMovieSize, t8Size);
        std::memset(varDst, 0, kVarBlockSize);
        std::memcpy(guestBuffer(rdram, data, kHudMovieSize), patched.data(), gow_menu_patch::kMovieSize);
        std::fprintf(stderr, "[gow-menu] FLP_HUD replaced in place by the patched movie (%u -> %u bytes; T8 %u bytes at %08x, T9 at %08x)\n",
                     kHudMovieSize, gow_menu_patch::kMovieSize, t8Size, t8Block, t9Block);
        g_state.movieData = data;
        g_state.t9Block = t9Block;
        g_state.t8Block = t8Block;
        g_state.varBlock = varBlock;
        // The loader can be interrupted by an EE checkpoint in the middle of the walk and resumed later: nothing may be
        // written into the header here. finishMovie() gives the movie its T8 and T9 tables once the walk is complete.
        sub_0014AE68_0x14ae68(rdram, ctx, runtime);
    }

    // EE thread, every VBlank: the page that is armed (PS2_PortPage) has its settings in the slots; a slot the movie changed goes to
    // its setting, a setting that changed elsewhere (F2, the other menu) goes to its slot. Whoever changed last wins. When the page
    // changes the slots are refilled from the new page's settings and nothing is carried over from the old page's values.
    void gowNativeMenuVBlank(uint8_t *rdram)
    {
        if (!g_state.ready)
            return;
        uint8_t *words = guestBuffer(rdram, g_state.varBlock, 4u * kVarCount);
        if (!words)
            return;
        auto word = [&](int i) { uint32_t v; std::memcpy(&v, words + 4 * i, 4); return v; };
        auto put = [&](int i, uint32_t v) { std::memcpy(words + 4 * i, &v, 4); };
        uint32_t page = word(0);
        if (page >= static_cast<uint32_t>(PageCount))
            page = PageNone;
        if (page != g_state.lastPage)
        {
            g_state.lastPage = page;
            for (int i = 0; i < kSlotCount; ++i)
            {
                const uint32_t v = i < kPageSlots[page] ? settingGet(static_cast<int>(page), i) : 0u;
                put(1 + i, v);
                g_state.last[i] = v;
            }
            return;
        }
        for (int i = 0; i < kPageSlots[page]; ++i)
        {
            uint32_t value = std::min(word(1 + i), settingMax(static_cast<int>(page), i));
            const uint32_t current = settingGet(static_cast<int>(page), i);
            if (word(1 + i) != g_state.last[i])
            {
                if (settingSet(static_cast<int>(page), i, value))
                    logSettingChange(static_cast<int>(page), i, value);
                else
                    value = current;
            }
            else if (value != current)
                value = current; // changed elsewhere (F2): show it in the menu
            put(1 + i, value);
            g_state.last[i] = value;
        }
    }

    // 0x1B8A48: reads and indexes msgs_en.txt. Sampled after the original returns (the tables exist by then).
    void gowDiagMenuMessages(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const bool entry = ctx->pc == 0x001B8A48u;
        sub_001B8A48_0x1b8a48(rdram, ctx, runtime);
        if (!entry || !diagEnabled())
            return;
        std::fprintf(stderr, "[gow-menu] message bank loaded: %u blocks, %u lines\n", readGuest32(rdram, kBankIndexCount),
                     readGuest32(rdram, kBankStringCount));
    }

    // 0x1B9358: a prompt (title lines, then up to five choices) is shown. a1 = block id in the message bank.
    void gowDiagMenuShow(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x001B9358u || !diagEnabled())
        {
            sub_001B9358_0x1b9358(rdram, ctx, runtime);
            return;
        }
        static unsigned samples = 0u;
        const uint32_t hud = GPR_U32(ctx, 4), id = GPR_U32(ctx, 5);
        if (samples++ < 64u)
            std::fprintf(stderr, "[gow-menu] prompt id=%u hud=%08x ra=%08x\n", id, hud, GPR_U32(ctx, 31));
        sub_001B9358_0x1b9358(rdram, ctx, runtime);
        if (samples <= 64u && guestBuffer(rdram, hud, kHudSizeNeeded))
        {
            // After the original: the five choice slots and the selection/event words it wrote.
            char text[3][40] = {};
            for (uint32_t i = 0; i < 3u; ++i)
                readString(rdram, readGuest32(rdram, hud + 0xACu + 4u * i), text[i], sizeof(text[i]));
            std::fprintf(stderr, "[gow-menu]   choices: '%s' '%s' '%s' event=%u default=%u\n", text[0], text[1], text[2],
                         readGuest32(rdram, hud + 0x208u), readGuest32(rdram, hud + 0x278u));
        }
    }

    // 0x1B7CE0: LoadMovie(hud, name). a1 points to the movie name.
    void gowDiagMenuMovie(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        if (ctx->pc != 0x001B7CE0u || !diagEnabled())
        {
            sub_001B7CE0_0x1b7ce0(rdram, ctx, runtime);
            return;
        }
        const uint32_t hud = GPR_U32(ctx, 4), name = GPR_U32(ctx, 5);
        char text[64] = "?";
        readString(rdram, name, text, sizeof(text));
        static unsigned samples = 0u;
        if (samples++ < 64u)
            std::fprintf(stderr, "[gow-menu] movie '%s' hud=%08x ra=%08x\n", text, hud, GPR_U32(ctx, 31));
        if (extensionRequested() && !g_state.attempted && hud == readGuest32(rdram, kHudPointer))
            prepareExtension(rdram, ctx, runtime, hud);
        if (extensionRequested())
            finishMovie(rdram);
        sub_001B7CE0_0x1b7ce0(rdram, ctx, runtime);
        validateHud(rdram);
    }
}
