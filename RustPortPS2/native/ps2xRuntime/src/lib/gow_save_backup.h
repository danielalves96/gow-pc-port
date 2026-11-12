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
