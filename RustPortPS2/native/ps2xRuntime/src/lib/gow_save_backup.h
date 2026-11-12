// GOW-Port: dated copies of the memory card (the save history the launcher's Saves page lists). One is made each time the
// game completes a save at an altar (src/game/gow_save.cpp), so a later bad save or a cut one never costs more than the
// progress since the previous altar. The card itself is untouched.
//
// Layout, next to the card image (`GOW_SAVE_BACKUPS_DIR` moves it):
//   <card folder>/save-backups/<YYYYMMDD-HHMMSS>[-N]/Mcd001.ps2   the copy (written last: its presence means "complete")
//   <card folder>/save-backups/<YYYYMMDD-HHMMSS>[-N]/backup.json  {"created", "created_unix", "source", "size", "card"}
// gow-launcher/crates/gow-core/src/saves.rs reads the same layout.
#pragma once

#include <string>
#include <vector>

struct GowSaveBackupResult
{
    bool ok = false;
    std::string directory; // the new backup folder (ok)
    std::string error;     // what failed (not ok)
};

// The card image slot 1 uses: GOW_MC0, or Mcd001.ps2 next to the ELF (as the IOP resolves it). Empty with GOW_MC0=0.
std::string gowSaveCardPath();

// Copies the card to a new dated folder. `source` is "auto" (a completed save) or "manual". Automatic copies beyond the
// newest GOW_SAVE_BACKUP_KEEP (default 30, 0 keeps all) are removed; manual copies and folders it did not write never are.
// Safe from any thread; one copy at a time.
GowSaveBackupResult gowSaveBackup(const char *source);

// ---- Loading a slot from outside the game's own menus ------------------------------------------------------------------
// The launcher's Saves page and the F2 menu ask for one slot of the card; src/game/gow_save_load.cpp (EE thread) takes the
// request and walks the game's own path to it (back to the main menu, Load, that slot). The launcher writes the request
// as a file next to the card, `<card folder>/load-request` (`slot=<0..3>` and `created_unix=<seconds>`), published with
// a rename; the F2 menu posts it in memory. gow-launcher/crates/gow-core/src/saves.rs writes the same file.

// One save of the card as the game holds it in memory (its slot index, read after the game checked the card).
struct GowSaveSlotInfo
{
    int slot = 0;            // 0..3, the order of the game's own list
    unsigned savePoint = 0;  // the altar (its thumbnail is TXR_SavePt<NN>, its name message 600 + NN)
    unsigned difficulty = 0; // 0 Mortal, 1 Hero, 2 Spartan, 3 God (message 3 + difficulty)
    float playSeconds = 0.0f;
    std::string place;          // the game's own name of the altar, when its message bank is loaded
    std::string difficultyName; // idem
};

// Any thread: asks for `slot` (0..3). A newer request replaces an older one that was not taken yet.
void gowSaveLoadRequest(int slot);
// EE thread: the pending request (memory first, then the launcher's file, polled at most twice a second), or -1.
int gowSaveLoadTakeRequest();
// `<card folder>/load-request`. Empty without a card.
std::string gowSaveLoadRequestPath();

// The slots the F2 menu lists: published by the EE thread, read by the window thread.
void gowSaveSlotsPublish(std::vector<GowSaveSlotInfo> slots);
std::vector<GowSaveSlotInfo> gowSaveSlotsSnapshot();
// `h:mm:ss`, as the game shows the play time.
std::string gowSavePlayTime(float seconds);
