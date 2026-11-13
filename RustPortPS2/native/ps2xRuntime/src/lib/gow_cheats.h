// GOW-Port: narrow public interface of the cheat engine (gow_cheats.cpp), for code that is not the F2 menu.
// The F2 overlay and any other front end (for example a menu of the game itself) operate on the same atomic state, so a
// change made through one is visible through the other. The effect itself (the health wrappers and the VBlank refill)
// is not duplicated: it stays in gow_cheats.cpp.
//
// Policy: when the engine is not installed (not God of War, RetroAchievements Hardcore active, or the health
// functions are missing) every setter refuses and every getter reports "off". This is the same switch that disables
// the F2 menu, so no front end can bypass the Hardcore restriction.
#pragma once

// True once gowCheatsInstall() succeeded: the cheats can be read and changed.
bool gowCheatsAvailable();

// Number of cheats of the F2 menu (the same list, in the same order: god_mode, one_hit_kill, infinite_magic, infinite_rage,
// max_orbs, all_upgrades, all_powers, max_meters, unlock_extras).
int gowCheatsCount();

// Current state of cheat `index` (false when unavailable or out of range).
bool gowCheatsGet(int index);

// Sets cheat `index`. Returns false, changing nothing, when the engine is unavailable or the index is out of range.
bool gowCheatsSet(int index, bool enabled);

// The id of cheat `index` ("god_mode"...), the value PC_CHEATS and the launcher use; null when out of range.
const char *gowCheatsId(int index);
