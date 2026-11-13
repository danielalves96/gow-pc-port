# Port Options of the pause menu (Cheats, In Game Overlay, Image Adjustments): patch tools

The menus of *God of War* are an Adobe-Flash-style movie (`FLP_HUD`, AVM1 bytecode) that the game loads from the PAK.
`GOW_NATIVE_CHEATS_MENU=1` hands the game a modified copy of that movie with one more pause-menu row, **Port Options**, between
*Options* and *Restart*; X on it opens a list (**Cheats**, **In Game Overlay**, **Image Adjustments** and Back), and each item opens a
page built with the same plates, fonts and handlers: one ON/OFF row per cheat of the F2 menu, one per line of the performance overlay
(FPS, GPU, CPU, RAM), or four sliders cloned from the game's volume rows (brightness, contrast, gamma, saturation) with Reset to
Default, or the frame rate (FPS limit as a two-value row, 60 / 120, cloned from Sound Mode, VSync, Early FINISH), and Back. The Options page is left as the game has it.
The **title menu** (the movie `FLP_Shell`, the one after Press Start) gets the same item and the same pages: a fifth item, **Port
Options**, after Treasures.
The research and the design are in [`docs/dev/NATIVE_MENU_RESEARCH.md`](../../../../docs/dev/NATIVE_MENU_RESEARCH.md).

The repository holds no part of the game: it holds a **delta** (`ps2xRuntime/src/game/gow_menu_patch.h`) that the runtime applies
to the movie it finds in memory, after checking its size and CRC32 (`gow_menu_patch.h` for the pause menu,
`gow_menu_patch_shell.h` for the title menu). These scripts regenerate those deltas from your own game.
Plain Python 3, no dependencies.

| Script | Does |
|---|---|
| `make_patch.py <PART1.PAK>` | The whole chain below; rewrites `gow_menu_patch.h` and `gow_menu_patch_shell.h`. Work files go to `build-gen/menu-patch/` (git-ignored). |
| `extract_hud.py` | Finds `FLP_HUD` in the PAK (or another movie by name: `extract_hud.py <PAK> <out> FLP_Shell`). |
| `flp_patch_cheats.py` | Builds the patched movie: parses the movie (`flp_model.py`), edits its tables and AVM1 actions (`avm1asm.py`, `flp_helpers.py`), and writes it back without the read-only tables T8 and T9 (they are supplied from blocks of their own at run time to stay inside the memory budget; the delta carries the patched T8 after the movie). Any unexpected shape of the input raises: nothing is patched blindly. The game's heap writes block pointers into `[0x1FF30, 0x20000)` of the buffer the pause movie is parsed in, so `flp_model.serialize(window=...)` turns that range into dead padding (zeros after the End action of a frame script); without it the result depends on the exact size of the movie (see the trap in `NATIVE_MENU_RESEARCH.md`). |
| `flp_patch_shell.py` | The same for the title menu (`FLP_Shell`): the fifth item, the list and the three pages as root frames 150 and up, one set of key handlers (X, Up, Down, Left, Right, Triangle) shared by all the pages. The patched movie does not fit the buffer the game loaded the retail one into, so it is written complete (tables included) and the runtime parses it in a block of its own. |
| `make_delta.py` | Turns *(retail movie, patched movie)* into copy/insert operations plus the checksums (`make_delta.py <orig> <patched> <header> shell` for the title menu). |

Run it again only if the page changes. Never commit the `FLP_HUD*.bin` files.
