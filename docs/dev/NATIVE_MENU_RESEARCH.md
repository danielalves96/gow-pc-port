# Original in-game menu research (SCUS-97399)

Status: **PASS for the core feature, with the limits listed in [Validation evidence](#10-validation-evidence)**. The same item and pages are
also in the **title menu** (section 5g). The
architecture of the original UI is recovered (sections 2 to 5e) and the pause menu now has a native **Port Options** entry (below
Options) that opens a list with two items: **Cheats**, a screen with **all nine cheats of the F2 menu** (each ON/OFF) and **Back**,
and **In Game Overlay**, a screen that switches the performance overlay's FPS, GPU, CPU and RAM lines on and off. Both are drawn by
the game's own movie engine with its own plates, font and handlers (sections 5f and 6). It is opt-in (`GOW_NATIVE_CHEATS_MENU=1`), the
cheats share state with the F2 cheat menu, the overlay lines with the launcher's switches (live, and kept for the next session: section 8, "Persistence"), and all of it is off
under RetroAchievements Hardcore. It was verified in the running game on the retail data, including a Release build made like the installer's final build
(see section 10). Confidence labels: CONFIRMED means read directly from the executable's instructions
or data and cross-checked by a second independent artifact or by a run; PROBABLE means one artifact plus a consistent
role; HYPOTHESIS means plausible only.

## 1. Environment and tools

| Item | Value |
|---|---|
| Executable | extracted `SCUS_973.99` (retail-compatible, CRC `D6385328`, SHA-256 `EC9397D2...AD71C`, see `ps2-tools verify-elf`) |
| Disc data | The data the port is meant for is the **retail USA disc** (`PART1.PAK`, `PART2.PAK`, `GODOFWAR.TOC`, ELF CRC `D6385328`), installed by the launcher in `D:\GOW_TESTE\game`. The repository's own `game/` folder holds the *Definitive Edition* ISO (`PART1.P4K`, modified ELF `ECF4D413...`): its movies and message bank differ from retail (see below). Findings marked *retail* were checked against `PART1.PAK`. |
| Ghidra | 12.2 PUBLIC (`C:\Users\danie\Ghidra`), JDK 25, headless |
| Ghidra project | `build-gen/ghidra-menu/GowMenu` (git-ignored). 4,801 functions. Language `MIPS:LE:64:64-32R6addr` |
| REA | MCP server `rea`, Ghidra provider **unavailable**: it requires Ghidra 12.1.x (verified 12.1.4) and 12.2 is installed (`unsupported_version`). Hopper: unsupported on Windows. IDA and Kuna: not configured. No REA analysis was possible. |
| Static analysis used instead | the disassembly in the comments of the generated recompiler C++ (`runner/*.cpp`, decoded by the R5900-aware `rabbitizer`) plus the ELF bytes, through `build-gen/menu-research/gowre.py` |
| Disc data analysis | `build-gen/menu-research/scanp4k.py`, `avm1.py` (AVM1 action disassembler) |

**Ghidra limitation (important).** Stock Ghidra has no R5900 language. The project uses a MIPS R6 variant, which cannot
decode the R5900 MMI and COP2 instructions (`lq`, `sq`, `pmfhl`...). Every function in the menu path that uses them
decompiles to `halt_baddata()`. The first revision of this document drew conclusions from that output; they were
replaced by the ones below. The names and comments of the identified functions are saved in the project by
`build-gen/ghidra-menu/NameMenuFunctions.java` (19 functions; naming does not depend on decoding). Next improvement:
import with an R5900 language module (community extension) so the decompiler is usable.

## 2. What the original menu is

The retail menus are **not** C++ screens, static item tables or a text-file menu. They are three layers:

1. **Flash-style movies** (AVM1 bytecode: `SetTarget`, `GotoLabel`, `Push`, `SetVariable`, `If`) drawn by a Flash-like
   player inside the EE code. The pause menu, Options page, save/load, death and prompt screens are movies. The layout,
   the row clips (`PauseMenu/WideScreen/On`, `PauseMenu/ButtonBack`...), the navigation and the text are movie content.
2. **A C++ "HUD object"** (pointer in `*0x29CB94`, at least `0x1560` bytes) that owns the movies (three slots at
   `+0x1558`) and a **table of ~190 named variables** at `+0x2BC` (entries of 0x18 bytes). The movies read and write
   those variables as `PS2_<name>` (`_level0/:PS2_WideScreen`, `PS2_Menu_Choice`, `PS2_SoundMode`...).
3. **The game's data** (`PART1.P4K`): movies (`<name>` and `FLP_<name>` assets) and the message bank `msgs_en.txt`.

`FSCommand:` (`0x144E40`) is the movie-to-game command channel; `PS2_*` variables are the shared state.

### Evidence for the layers

* The data holds raw AVM1 action streams (`96 .. 00` `Push`, `8B` `SetTarget`, `8C` `GotoLabel`, `1D` `SetVariable`).
  Disassembling them (`avm1.py`) shows the Options page: it reads `_Level0/:PS2_WideScreen` and runs
  `SetTarget "WideScreen/On"; GotoLabel "Wait"`, sets `_Level0/:PauseOptions_State`, reads `PS2_ProgressiveScan`, etc.
* The movie vocabulary (clip paths) lists the Options rows: `Vibration`, `MusicVolume`, `SFXVolume`, `SoundMode`
  (`Mono/Stereo/Surround`), `WideScreen`, `ProgressiveScan`, `Deflicker`, `Controller`/`ControllerStyle` and `ButtonBack`;
  the pause menu: `Continue`, `Options`, `Restart`, `Exit`.
* The ELF registers exactly those names (`0x1B5D70`: `Vibration`, `WideScreen`, `ProgressiveScan`, `DeFlicker`,
  `SoundMode`, `MusicVolume`, `SoundVolume`, `Sound1..4`, `Menu_Choice`, `PauseMenu_Event`...) and logs `SetTarget "%s"` /
  `GotoLabel "%s" == %d` in `0x1452F8` (the interpreter).
* The visible labels ("OPTIONS", "VIBRATION") are **not** ASCII anywhere in the ELF or in `PART1.P4K` (searched
  `OPTIONS`, `VIBRATION`, `RESUME`, case-insensitive variants and UTF-16). They are glyph content of the movies, so
  they are not editable text.
* *Retail check* (`PART1.PAK`): `PauseMenu/ButtonBack`, `PauseMenu/Vibration`, `PauseMenu/WideScreen`, `PauseMenu/Deflicker`
  and `PauseOptions_State` are present (offsets `0x2C4B41`..`0x2FC6B1`, a ~230 KB region near the start of the file);
  `ControllerStyle` and `PCX_ControllerType` are **absent**, so the `Controller` style row is an addition of the
  Definitive Edition. `CheatWadSelect` is present in retail (a developer leftover). Any movie patch must be validated
  against the movie it receives, never assumed, because the two editions already differ.
* Extracted `msgs_en.txt` figures (383 blocks, ids 1..3041) come from the Definitive Edition file. At runtime on retail the
  bank loaded at the title holds **23 blocks / 23 lines**.

## 3. Function map

"Function map entry" is the row of `RustPortPS2/games/scus-97399/funcmap.csv` named `sub_<ADDR>` (8 hex digits, upper
case) and the generated function is `sub_<ADDR>_0x<addr>` in `runner/`. Disassembly was read from those generated
files. Runtime evidence is in section 10.

| Address | Proposed name | Purpose | Callers / callees | Confidence |
|---|---|---|---|---|
| `0x1B5D70` | `Hud_Construct` | Constructor of the HUD object. Zeroes the three movie slots (`+0x1558`) and the variable table, registers ~190 variables, then calls `0x1B9BC8`, `0x1BA220`, `0x170588`, `0x1B77D8`, `0x1B78C8`. Called from `0x21C788`. | callee `0x1B5CE0`, `0x1B8A48` | CONFIRMED (instruction-level, matches the variable names the movies use) |
| `0x1B5CE0` | `Hud_RegisterVariable` | `a0=hud a1=type a2=name a3=flag t0=storage address t1=default`. Writes entry `+0x2BC + slot*0x18`: `+0 name`, `+4 type`, `+8 storage`, `+0xC flag`, `+0x10 default (8 bytes)`. Slot from `0x1B5C70` (max `0xBF`). | caller `0x1B5D70` | CONFIRMED |
| `0x1B7CE0` | `Hud_LoadMovie` | `a0=hud a1=movie name`. Reuses a slot whose movie has that name, otherwise `0x14C048` creates it in a free slot (`hud+0x1558+4n`, 3 slots), formats the `PS2_` prefix (`0x14A8A0`) and binds the HUD variables (`0x1B79E0`). | callers `0x1BBE28`, `0x1BBE78`, `0x1BDB58` | CONFIRMED |
| `0x14C048` | `Flash_LoadMovieAsset` | `a0=name`. Looks up assets `<name>` and `FLP_<name>` with `0x17A658`, then builds the movie through class vtables at `0x32E848`. | no direct `jal` caller (`0x14C258` is a thin wrapper; reached through function pointers) | CONFIRMED |
| `0x17A658` | `Asset_Find` | Find a loaded resource by name. Also used for `msgs_en.txt`. | many | PROBABLE |
| `0x14AE68` / `0x14BC58` | `Flash_BuildMovie` / `Flash_FixupMovie` | **`0x14AE68` is hooked by the Cheats page (section 6).** Walk the movie buffer (in place for the HUD movie, resource flag `0x2000`), rewriting pointer fields (helpers `0x14B680`, `0x14B6C8`, `0x14B890`, `0x14B8C8`, `0x14B910`, `0x14B948`, `0x14B9F8`, `0x14BB20`, `0x14BB58`); resolve asset references with `0x17A658`. Format decoded in 5b. | | CONFIRMED (cursor reproduced byte-exact on two files) |
| `0x1452F8` | `Flash_ExecuteActions` | The AVM1 interpreter (3,528 bytes). Strings `SetTarget "%s"`, `GotoLabel "%s" == %d`, `GotoExpression`. | called by queue drain `0x146158` | PROBABLE |
| `0x146158` / `0x1460C0` | `Flash_QueueDrain` / `Flash_QueueAppend` | Player action queue (already wrapped by `GOW_FLASHQ_WATCH`). | | CONFIRMED (repo) |
| `0x144E40` | `Flash_FSCommand` | Builds `FSCommand:` and `FS_Result__%s_%d`. | | PROBABLE |
| `0x1B8A48` | `Messages_Load` | Reads `msgs_en.txt` (`0x2B5698`): tokenizes lines (`0x28D408`), line `*N*` starts block N (`strtol` `0x291078`), builds `*0x29CBA4` (char* per line, 0x1900 bytes = 1600 lines) and `*0x29CBA0` (`{u16 id, u16 first line}`, 0xA04 bytes = 641 entries), counters `0x29CBA8` / `0x29CBAC`. It also tears down any prompt in progress. | caller `0x1B5D70` (only caller) | CONFIRMED |
| `0x1B8CE0` | `Messages_FindBlock` | `a0=id a1=out first line`; binary search (`0x28B198`, comparator `0x1B8CD0`) of the index; returns the line count (next block's first line minus this one). | callers `0x1B8D88 0x1B8E88 0x1B9358 0x1B9780 0x1B9828 0x1B9BC8 0x1B9F08 0x1BA220 0x1BA8F8 0x1DB2F0` | CONFIRMED |
| `0x1B9358` | `Hud_ShowPrompt` | `a0=hud a1=block id`. Splits the block at the `----` separator line: lines before it are `MenuTemplate_Line1..3`, lines after it are `MenuTemplate_Menu1..5` (a leading `*` marks the default choice). Writes `+0x208` (`MenuTemplate_Event` = lines*10 + choices) and `+0x278` (`Menu_Choice` = default). If the id is unknown it shows a debug `Menu %d` with `Yes`/`No`. | caller `0x21D958` | CONFIRMED |
| `0x21D958` | `Hud_PumpEvents` | Pops one queued UI event (`0x21DC48`) and dispatches: message template (`0x1B8DF0`), prompt (`0x1B9358`), meters (`+0x14`, `+0x18`), pick-up info. | caller `0x21F3C8` | PROBABLE |
| `0x1BA4F8` | `Hud_UpdateTitle` (guess) | Bonus/title menu update (`SND_MM_SELECT`, `Bonus20`, `SND_M_TIT1E1_2`). Not the Options path. | | HYPOTHESIS |

Arguments follow the MIPS convention (`a0..a3`, then `t0..`), the return value is in `v0`.

## 4. Reconstructed structures

**HUD object** (`*0x29CB94`), only the parts needed:

| Offset | Content | Evidence |
|---|---|---|
| `+0x10` | pointer set by the constructor (`0x2BA4B0`) | `0x1B5DA8` |
| `+0x14..` | meter variables (`HealthMeter_Value`...) | registration table |
| `+0xA0..+0xA8` | `MenuTemplate_Line1..3` (char*) | `0x1B9494` |
| `+0xAC..+0xBC` | `MenuTemplate_Menu1..5` (char*) | `0x1B9504`..`0x1B95B0` |
| `+0x1BC` | `EnableButtons` | registration |
| `+0x208` | `MenuTemplate_Event` | `0x1B93B0`, `0x1B9650` |
| `+0x254`, `+0x258`, `+0x25C`, `+0x260` | `PauseMenu_Event`, `PSMMenu_Event`, `DeadMenu_Event`, `FailedMenu_Event` | registration |
| `+0x278` | `Menu_Choice` (shared with the movie as `PS2_Menu_Choice`) | `0x1B93B4`, `0x1B963C` |
| `+0x27C` / `+0x280` / `+0x284` | `Done` / `PauseDone` / `PSMDone` | registration |
| `+0x290..+0x2B8` | `MusicVolume`, `SoundVolume`, `Vibration` (`+0x298`), `WideScreen` (`+0x29C`), `ProgressiveScan` (`+0x2A0`), `DeFlicker` (`+0x2A4`), `SoundMode` (`+0x2A8`), `Sound1..4` | registration |
| `+0x2BC` | variable table: `0xBF` entries of `{char *name, u32 type, void *storage, u32 flag, u64 default}` | `0x1B5D30..0x1B5D4C` |
| `+0x1528`, `+0x152C`, `+0x1530` | prompt/message-in-progress state words | `0x1B9374`, `0x1B93A4`, `0x1B93C8` |
| `+0x1558` | three movie slots | `0x1B7D08`, `0x1B8440` |

The names in the table are registered with `type=7` except `DropShadowOffset` (`type=1`).

**Message bank**: `*0x29CBA4` char* array (up to 1600), `*0x29CBA0` index `{u16 id, u16 first}` (up to 641, sorted by
id because it is searched with a binary search), counters `0x29CBA8` (blocks) and `0x29CBAC` (lines). Source file
format (verified on the disc data, 383 blocks, ids 1..3041): `*N*` header line, then text lines, blank line between
blocks; `----` separates prompt title from choices. The file holds in-game messages and Yes/No prompts (ids 1001..1019),
not the Options labels.

## 5. Flows

* **Menu creation**: `Hud_Construct` registers the variables and binds the movie slots; `Hud_LoadMovie(name)` opens a
  movie (pause, save/load, dead...). The movie's first frame actions initialize its states (`PauseMenu_State`,
  `PauseOptions_State`) and read the `PS2_*` variables to pose each row clip (`WideScreen/On` vs `/Off`).
* **Selection and input**: handled in movie content (frame labels and `*_State` variables such as `PauseOptions_State`,
  `PauseAllowScrollLR`, `Pause_Check`). The C++ side contributes the `PS2_*` values and receives `FSCommand`s. The exact
  pad path from libpad to the movie is not traced yet.
* **Transitions**: movie state variables (`PauseMenu_State`...) and `GotoLabel` on clip paths. The prompt path is the
  native one: an event reaches `Hud_PumpEvents` → `Hud_ShowPrompt`; the movie then edits `PS2_Menu_Choice`.
* **Text**: prompt text comes from the message bank into `MenuTemplate_*`; row labels of Options are movie glyphs.

## 5b. The FLP movie format (decoded)

Status: the container, every table and every action script of `FLP_HUD` are decoded and the decoder reproduces the
loader's own cursor: on both the retail file (479,303 bytes) and the Definitive Edition file (498,625 bytes) the cursor
ends exactly at the end of the file. Tools (local, git-ignored): `build-gen/menu-research/wad.py`, `flp.py`,
`flp_sprites.py`, `avm1.py`. Confidence: CONFIRMED for the layout (byte-exact reproduction of the loader walk);
PROBABLE for the meaning of the fields that the loader only skips.

**WAD record** (retail `PART1.PAK`, `R_PERM.WAD` is the first 0x378530 bytes): `[u32 tag][u32 size][char name[24]]`
followed by `size` bytes, the next record at the next multiple of 16. Tag low word `0x1E` is an item and the type is in
the high word (`0x8001E` = FLP movie, `0x6001E` = material, `0x3001E` = texture...). Tag `0x18` carries a value in `size`
and has no payload. Examples: `FLP_HUD` (tag `0x8001E`, 479,303 bytes at file offset `0x2B8160`) and `msgs_en.txt`
(tag `0x6F`, 36,553 bytes in the Definitive Edition; retail has its own at `0x32D1D0`).

**FLP file** (the loader copies it and only rewrites pointer fields, `0x14BC58` and its helpers). A `0x60` byte header
holds counts; the pointer fields it contains are stale tool addresses that the loader overwrites. After the header the
tables follow back to back, each aligned to 4, and **the child arrays and blobs of every element sit right after the
table that owns them** (a running cursor):

| Header field | Table | Element | Children placed after the table |
|---|---|---|---|
| `+0x0C` u32 | T0 | 4 bytes | none |
| `+0x14` u32 | T1 | 8 bytes | per element `u16@+2` entries of 8 bytes (ptr `@+4`) |
| `+0x1C` u32 | T2 | `0x24` | flag-dependent arrays (flags `u16@+0xC`: bit 1 and bit 2 add 8-byte arrays of `u32@+0` entries), a `u16` array, and a `0x200` or `n*2` byte block |
| `+0x24` u32 | **T3** | `0x24` | a raw byte blob of `u32@+0x14` bytes (ptr `@+0x18`) |
| `+0x2C` u32 | T4 | `0x20` | none |
| `+0x34` u32 | T5 | `0x0C` | one `0x18` timeline object and `u16@+8` entries of 16 bytes, each followed by a `u32@+0` byte blob |
| `+0x3C` u32 | **T6** | `0x18` | the **timeline object** (clips, 381 in retail) |
| `+0x44` | **T7** | one `0x18` | the **root timeline** |
| `+0x48` u16 | T8 | `0x14` | none |
| `+0x50` u16 | T9 | 8 | none |
| `+0x58` u16 | T10 | 1 | byte blob |

**Timeline object** (`0x14B9F8`, 0x18 bytes): `u16@+2` operations (8-byte records, each with `u16@+2` entries of `0xA`
bytes: placements, decoded in 5d) and `u16@+4` frames (`0xC` bytes each, `u16@+2` actions per frame, each action
an 8-byte `{u32 size, ptr}` pointing to a **raw AVM1 byte blob**). So every frame script is a standalone blob that the
interpreter (`0x1452F8`) runs; names such as `PauseMenu/Options` are strings inside those blobs.

Retail `FLP_HUD`: 381 clips (`T6`), the pause-menu clip is `T6[233]` (18 frames, 26 operations) and the root timeline
`T7` has 52 frames. The Definitive Edition file differs in table counts (T0 979 vs 936, T1 240 vs 220, T6 399 vs 381,
T9 2906 vs 1496), which is the footprint of its extra Options row and other edits.

## 5c. The Options page state machine (decoded, retail)

The visual and navigation logic of the Options page is **AVM1 content**, not C++:

* `T6[233]` (clip `PauseMenu`), frame 5: the Options page initialization (`TextOptions`, then it poses each row clip
  from `PS2_WideScreen`, `PS2_ProgressiveScan`, `PS2_DeFlicker`, `PS2_SoundMode`, `PS2_MusicVolume`, `PS2_SoundVolume`,
  `PS2_Vibration`) and sets `_Level0/:PauseOptions_State = 1`. Frame 3/4 is the pause top level (`PauseMenu_State = 1`,
  `PauseMenu/Continue` on), frame 7 the exit confirmation, frame 8 the restart confirmation, frame 10 the controller
  screen (`PauseConfig_State`).
* `T7` (root), frame 9 (ChainUp: `SetTarget`/`GotoLabel`, play sound `SND_MM_NAV_UPDN` through `PS2_Sound1`) and frame
  10 (ChainDown): the navigation. `PauseMenu_State` selects the pause level (1 Continue, 2 Options, 4 Exit...) and
  **`PauseOptions_State` selects the Options row**: row clips are highlighted with `GotoLabel 'On'` / `'Off'`, and the state
  changes to the neighbor.

| `PauseOptions_State` | Row clip (`PauseMenu/...`) | Up goes to | Down goes to |
|---|---|---|---|
| 1 | `WideScreen` | 9 | 2 |
| 2 | `ProgressiveScan` | 1 | 3 |
| 3 | `Deflicker` | 2 | 4 |
| 4 | `MusicVolume` | 3 | 5 |
| 5 | `SFXVolume` | 4 | 6 |
| 6 | `SoundMode` | 5 | 7 |
| 7 | `Controller` | 6 | 8 |
| 8 | `Vibration` | 7 | 9 |
| 9 | `Return` (back) | 8 | 1 |

(0 is the pause top level.) Confidence: CONFIRMED for the state graph (read from the up and down scripts, which agree).
The left/right handlers are root frames 11 and 12 (they test `PauseOptions_State` and call `<Row>Call`); the X handler is
`T5[3]` entry 0 (release) and the Triangle handler `T5[4]` entry 0. All of them were decoded for the patch (5f).

**What a CHEATS row needs in this design** (derived from the above; implemented, see 5f): (1) a new row clip
`PauseMenu/Cheats` with `On`/`Off` labels and its glyphs (the retail Options labels are glyph shapes inside the clips);
(2) a new state number inserted into both chain scripts (between 8 and 9) and into the wrap-around of state 9;
(3) the confirm handler for the new state; (4) a second page. All four are movie content. The Definitive Edition did
the same for its extra row, which is why its file is larger.

## 5d. Characters, layers and the anatomy of an Options row (decoded, retail)

Tools: `build-gen/menu-research/flp_clip.py <FLP.bin> <T6 index>`. Confidence: CONFIRMED for the dictionary and the layer
records (they resolve every instance name of the pause clip consistently); PROBABLE for the two numeric fields of a placement.

* **`T0` is the character dictionary**: 936 entries of `{u16 type, u16 index}`. A character id used anywhere in the movie
  indexes it. Types: `7` = clip, index into `T6` (381); `1` = `T1` (220); `4` = `T3` (199); `5` = `T4` (82); `6` = `T5` (11);
  `3` = `T2` (1); `0` = empty (42). Examples: id `0x211` (`PauseMenu` in the root) = `T6[233]`; `0x1D7` (`WideScreen`) = `T6[213]`.
* **`T10` is the name pool** (byte offsets, NUL-terminated): instance names, variable paths (`_level0/:PS2_...`), frame
  labels (`On`, `Off`, `Wait`, `None`) and the texture names `TXR_HUD000...` (first strings of the pool).
* **Timeline object** (`T6[n]`, `T7`): header `u16@+0` and `u16@+2` count the **layers**, `u16@+4` the frames. A layer is
  `{u16 depth, u16 entries, ptr}` and each 10-byte entry is `{u16 frame, u16 character id, u16 a, u16 b, u16 name offset (0xFFFF none)}`:
  place or update that character at that frame. `a` and `b` index per-placement transform/color data (unique `a` per
  placement, probably into `T8`/`T9`); an entry with character `0` ends the instance. A frame record is `{u16 frame, u16 nActions,
  ptr, u16 label name offset, u16 flags}`: the frame labels that `GotoLabel` targets come from here.
* **The pause clip `T6[233]`** has 26 layers. Named instances (depth: character): `ButtonBack`, `ButtonSelectBack`,
  `ExitFailsafeMenu`, `ControllerText`, `Exit`, `Return` (`0x1B5`, `T6[195]`), `ExitOlympus`, `Controller` (`0x1B7`, `T6[196]`),
  `Restart`, `SoundMode` (`0x1C1`, `T6[201]`), `Options`, `Continue`, `TextPause`, `SFXVolume` (`T6[206]`), `MusicVolume`
  (`T6[208]`), `Deflicker` (`T6[211]`), `ProgressiveScan` (`T6[212]`), `WideScreen` (`T6[213]`), `Vibration` (`T6[215]`),
  `TextOptions`.
* **Anatomy of a value row** (`WideScreen`, `T6[213]`, 19 frames, 5 layers): a label object `T3[73]`, two value clips `Off`
  (`T6[209]`) and `On` (`T6[210]`), and two `T6[200]` arrow instances `ArrowLeft`/`ArrowRight`. Frame labels: `On` at frame 1,
  `Wait` at 9, `Off` at 10, `None` at 18 (the highlight animation that the navigation scripts select with `GotoLabel`).
  **Anatomy of an action row** (`Controller`, `T6[196]`): one layer with a single `T3[62]` label and the same four frame
  labels. This is the natural model for a CHEATS row (it opens another screen and has no value).

**Label art (resolved in 5e).** The visible word of a row is a `T3` object (type 4: `0x24` byte element plus a blob of 62
to 558 bytes; median 82). The blob starts with a small header and an RGBA color (`cc cc cc ff`) followed by 16-bit
coordinate pairs, so it is geometry (quads or polygons) and not a string: the glyph shapes or texture rectangles of the
word are baked into the movie. How a `T3` blob is interpreted (and whether the word comes from a `TXR_HUD###` texture
atlas or from polygons) is decided by the renderer, which has not been traced. The format is decoded in 5e: a label is a text object in the HUD font, so a new word only needs a new glyph run.

## 5e. Static text (T3) and the HUD font (decoded, retail)

Source: the per-layer evaluator `0x147520` (type switch through the jump table at `0x2B1C20`) and the T3 renderer
`0x147BF8`. Confidence: CONFIRMED for the T3 stream layout (the `WideScreen` label of 62 bytes is consumed exactly:
13 + 1 + 12 x 4) and for the glyph mapping (all 9 distinct letters of the label resolve through the font table).

* **Character type dispatch** (`0x147520`, `T0` type): 1 = `T1` shape (`0x147870`, direct draw through `0x149FA0`),
  2 = `0x147BB0`, 3 = `T2` (`0x147B9C`), **4 = `T3` text** (node vtable `0x2BF4A0`, draw `0x147BF8`), 5 = `T4`
  (node vtable `0x2BF480`, `0x148458`), 6 = `T5` (node vtable `0x2BF460`), 7 = clip (`0x147A60`). Node vtables hold the
  destructor at slot 3 (`0x148330`, `0x1490D8`, `0x23B790`, `0x23B6A0` for the root).
* **T3 blob = a stream of records.** A record whose first byte has bit `0x80` set is a **style record**:
  `flags | [bit 0x08] u16 font (dictionary id of a T2) , u16 scale | [bit 0x04] 4 bytes color (R,G,B,A; the renderer adds 1 to each) |
  [bit 0x02] s16 x offset | [bit 0x01] s16 y offset`. Any other first byte is a **glyph run**: the byte is the count `n`, followed by `n`
  entries of `{u16 glyph index, s16 advance}`. The run is drawn twice with two transforms (PROBABLE: the second pass is a
  shadow or outline; not verified on screen). The `WideScreen` label is `8F | 14 00 | D0 02 | CC CC CC FF | 01 00 | 1B 02 | 0C | 12 glyphs`.
* **The font is `T2[0]`** (one font in the retail HUD movie): `n = 78` glyphs, flags `5`. Its arrays: `ptr@0x10` and `ptr@0x14`
  (8-byte glyph entries, two variants chosen by comparing a float at `+0x20` with the transform scale; PROBABLE: level of detail),
  `ptr@0x18` = `n` u16 **advances**, `ptr@0x1C` = a `0x200` byte block of 256 u16: the **character to glyph map**. Map for
  ASCII: `A..Z` -> 26..51, `a..z` -> 52..77 (small capitals), space -> 0, digits and punctuation have glyphs; 255 entries are non-zero.
* **Advance in a label** = font advance x `scale / 1024` (scale `0x2D0` = 720, so x0.703), within 1 unit (for example glyph 48
  has font advance 1153 and label advance 811).
* The retail label `WideScreen` is therefore the text "Wide Screen" written with one capital and small capitals
  (`W`=48, `ide`=60,55,56, space=0, `S`=44, `creen`), followed by one more glyph (22).

**Consequence**: a CHEATS label does **not** need new art. It is a T3 text object in an existing font: copy the style record of an
existing label and emit a glyph run for the letters needed (`C`=28 then lower-case `heats` = 59,56,52,71,70), advances
from the font table. Letters must exist in the font, and all 94 printable ASCII characters have glyphs.

## 5f. Port Options, Cheats, In Game Overlay and Image Adjustments in the pause menu (implemented; see section 10 for what was verified)

Three levels, all inside the `PauseMenu` clip (`T6[233]`) and its handlers; the Options page is **left exactly as the game has it**.

| Level | What the player sees | Where it lives |
|---|---|---|
| 1. Pause menu | Continue, Options, **Port Options**, Restart from last checkpoint, Quit Game | `PauseMenu_State` **6** is the new row; the numbers of the others (1 Continue, 2 Options, 3 Restart, 4 Exit, 5 Exit Olympus) are unchanged |
| 2. Port Options list | Banner "PORT OPTIONS", **Cheats**, **In Game Overlay**, **Image Adjustments** (first three rows), **Back** (last row) | frames 26 (init, label `PortOptions`) and 27 (stop); sub-state `PausePort_State` 1 = Cheats, 2 = In Game Overlay, 3 = Image Adjustments, 4 = Back |
| 3a. Cheats | Banner "CHEATS", nine ON/OFF rows (the cheats of the F2 menu) and **Back** | frames 24 (init, label `Cheats`) and 25 (stop); `PausePort_State` 5 and `PauseCheats_State` 1..9 (+10 for Back) |
| 3b. In Game Overlay | Banner "IN GAME OVERLAY", four ON/OFF rows (**FPS Counter**, **GPU Stats**, **CPU Stats**, **RAM Usage**) and **Back** right below them | frames 28 (init, label `Overlay`) and 29 (stop); `PausePort_State` 6 and `PauseOverlay_State` 1..4 (+5 for Back) |
| 3c. Image Adjustments | Banner "IMAGE ADJUSTMENTS" (font scale 540), four sliders (**Brightness**, **Contrast**, **Gamma**, **Saturation**), a free row, **Reset to Default**, **Back** | frames 30 (init, label `Image`) and 31 (stop); `PausePort_State` 7 and `PauseImage_State` 1..4 sliders, 5 Reset, 6 Back |

`PausePort_State` is 0 while the highlight is on the Port Options row of the pause menu. Navigation: Up/Down move the highlight
through Continue, Options, Port Options, Restart, Exit (the step Options to Restart and the step back now stop at Port Options);
X on Port Options opens the list, X on Cheats, In Game Overlay or Image Adjustments opens that page, X on a row toggles it (also
Left/Right; on a slider Left and Right move one step, X does nothing, like the game's volume rows), X on Reset to Default puts every
slider back to its middle step, X on Back
goes up one level (from the list the pause menu comes back with Continue highlighted, like "Return to Pause Menu" does; from a page
it returns to the list, with the highlight on the item the page was opened from: `PausePort_Return`, set by the page and consumed by
the list's init). Nine cheats and Back fit one page (the same ten-row density as the Options page), so no scrolling or second page
was needed; what limits the number of rows is the memory of the movie (section 6, item 4), not the screen.

Details that were learned the hard way:

* **Pause menu rows.** The rows sit 1200 twips apart (Continue -2000, Options -800, Restart 400, Exit 1600 on the normal frame
  5). Port Options takes 400 and Restart and Exit move down one row (1600, 2800). The "Olympus" variant of the menu (frame 6,
  used when `PauseOlympus_State` is 1) already has five rows, so it is squeezed to six rows 1040 twips apart. The T8 entries of
  those placements are copied before they are edited, because some are shared with other clips.
* **Shared layers.** The layers of the pause rows are reused for the Options rows at other frames: depth 22 is `Exit` at frames
  5 and 6 and `Return` at 7 and 8, depth 24 is `ExitOlympus` and `Controller`, depth 26 is `Restart` and `SoundMode`. A template
  for a new row must therefore be taken from the entries of the Options-page frames 7 and 8 only.
* **Centering.** A label is centered by the x offset of its T3 element (s16 at `+0x10`): `1438 - width / 2`, in the units of the
  glyph advances (checked on six labels, at scale 720 and at 1000). A new label gets its own offset, so the new rows ("Port Options",
  "Cheats", "In Game Overlay", "Back") and banners need no padding. The value rows keep the right-aligned layout of the "Progressive
  Scan:" row they are cloned from (names end on one line, Off/On form a column), moved 340 twips left so the block is centered near
  x = 675 px. The banners are drawn at a smaller font scale where the text would overflow the plate (the style's vertical offset
  follows the scale): "Port Options" at 800, "In Game Overlay" at 620, which makes both 5,6xx advance units wide (the stock
  "Options" banner is 4,212 at 1024).
* **The font has small capitals.** A lower-case letter is drawn as a smaller capital, so the generator writes every word in Title
  case, except all-caps words of two or more letters ("FPS", "GPU", "CPU", "RAM"), which keep their capitals; otherwise "FPS Counter"
  would read "Fps Counter" (`display_chars` in `flp_patch_cheats.py`).
* **Pages are frames.** A frame shows only the layers placed at that frame, so each page re-places the plate layers (depths 1, 2,
  7, 18, 21, copied from frames 6/5/5/7/7) and the `ButtonBack` hint at frame 24, and **the layers of a page must be removed by an
  entry at the first frame of the next page** (the Cheats layers carry a removal entry at 26, where the list begins, and the list
  layers one at 28, where the In Game Overlay page begins; without it the rows of the previous page stayed on the screen under the
  next one). A jump backwards needs no removal. The clip is four frames longer than the original (header length 30). Frames 20 and
  21 were tried first and collide with the `Animate_Off` animation.
* **Handlers patched (AVM1):** root frames 9, 10, 11, 12 (Up, Down, Right, Left), the X release handler (`T5[3]` entry 0) and the
  Triangle release handler (`T5[4]` entry 0) each get a block for `PauseMenu_State == 6`, inserted before the test of state 3 (before
  the test of state 2 for Left/Right, which have none for 3). Stale sub-states cannot survive: the steps that arrive at the Port
  Options row clear `PausePort_State`, and `PauseReset` clears `PausePort_State`, `PauseCheats_State`, `PauseOverlay_State` and
  `PausePort_Return`. The page inits pose every row from its variable, so a page opens showing the current state, also for a cheat set
  from F2 or `PC_CHEATS`, or an overlay line set by the launcher.
* **Toggles.** `<Name>Call` (nine of them, for the cheats) is a copy of `WideScreenCall` that flips `PS2_<Name>` and poses the
  `On`/`Off` clips, in free root frame numbers 49..57. The four overlay rows have **no subroutine**: the free numbers between the
  last used frame (48) and the next one (59) are 49..58, so only one is left; the frames at 62..93 are free numbers too, but the
  root timeline may play through that stretch, where a subroutine would run as an ordinary frame script, so they were not used.
  Instead the body of `WideScreenCall` is cloned in line (names replaced, its final `End` dropped, the jumps that went to it going
  to the handler's end) in the X and Left/Right blocks of the overlay page (`toggle_inline`).
* **Sliders (Image Adjustments).** The game's own volume rows are the model: `T6[208]` (Music Volume) is a label `T3[68]` plus a
  `MusicBar` clip (`T6[207]`) of 11 frames, `Music0`..`Music10`, and its variable holds the level 0..10 (the bar is posed with
  `GotoFrame2(level + 1)`, frames count from 1; Right adds one while `level < 10`, Left subtracts one while `0 < level`). The four
  rows are clones of `T6[208]` with their own label (the bar clip is shared, each row has its own instance, named `MusicBar` as in the
  original). **Resolution.** The first version used the level itself, so one press changed a setting by 10 %, too coarse for brightness
  and gamma. Now the variable is the Shade Boost **percentage** (0..100, 50 = no change) and a press moves it by `IMAGE_STEP` = 2 (the
  Right/Left blocks of `slider_step`: test against the limit, add or subtract, clamp to 0 or 100 because a start value from the launcher
  need not be a multiple of 2, then the sound of the SFX row, `SND_MM_SFX_SLIDER`); the bar is posed at `value / 10 + 1` (new opcode
  `Divide` in `avm1asm.py`), so it has 11 positions and moves once per five presses while the picture changes with every press (the
  fraction of the division is left to the frame jump of the engine, measured in the game, see section 10). The slider layers are cloned
  from the Music Volume row's entries at frames 7 and 8 (its T8 placement is copied and moved to the page's row, `IMAGE_TX` = -400 centers
  the block); the page init poses each bar from its variable. **Why a slider and not a number:** the font has no dynamic text, a number
  would need one clip per value; the bar already exists and has the highlight animation. Reset to Default is an action row cloned from
  "Controller": it sets the four variables to 50 and poses the bars (`reset_sliders`).
* **Triangle.** Back is one level on every page: Triangle on a value page goes to the list, on the list to the pause menu, on the
  pause menu closes it (blocks `tri` of `port_body`, which only run for `PauseMenu_State == 6`). For a while a tap went back three
  levels at once, in the **stock** Options page as well. It was not the movie, the pad word or the press length (0.03 s did it too, and
  the log showed a single Triangle read): it was the **input buffer of Agile combat** (`gow_animation_cancel.cpp`, `GOW_CANCEL`, on in
  the launcher), which re-sends a square or triangle press the game did not accept as an attack, up to three times over 0.35 s. In the
  pause nothing accepts it, so the menu got the press again. The buffer now only acts in gameplay (game state 11).

Everything is produced by `RustPortPS2/native/tools/menu/flp_patch_cheats.py`, which derives every index, depth and offset
from the movie it receives and raises on any unexpected shape (counts T0 936, T1 220, T3 199, T6 381, free frame numbers, one
test of each `PauseMenu_State` value, the highlight steps it rewires...).

## 5g. Port Options in the title menu (`FLP_Shell`; implemented, verified in the Release build)

The menu after Press Start is another movie, `FLP_Shell` (retail: 166,972 bytes, CRC32 `5D23C6D3`, 373 characters, 113 texts, 135
clips, 40 key handler records, 150 root frames), built by the same `Flash_BuildMovie` (`0x14AE68`, resource flags `0x2008`: parsed in
place) while the game boots, right after `FLP_HUD`. It uses the HUD object's variables, so the `PS2_<name>` switches registered for the
pause menu pages are the ones the title pages read and write (one set of switches, two menus). Unlike `FLP_HUD`, the buffer in memory
is byte for byte the file (no volatile window; checked with `GOW_MENU_DUMP_MOVIE`).

* **The title menu shows one item at a time** (New Game, Load, Options, Treasures; `ShellMenu_State` 1..4) with an up and a down arrow
  clip (`UpArrowMain/ArrowUp`, `DownArrowMain/ArrowDown`; `GotoFrame 0` hides, `GotoFrame 1` + `Play` shows). Each item is a clip with the
  labels `UpOn`, `UpOff`, `DownOn`, `DownOff` (`NewGame` also has `On`, the item shown at start) placed at the same transform (`T8[877]`),
  so a fifth item needs no layout work: the new row, `PortMain`, is a clone of the Options row clip (`T6[5]`) with a new text, and a root
  layer cloned from the Options row layer (frames 2..13). `OpeningUp` / `OpeningDown` (root frames labeled so, called by the up / down
  handlers) got a block for state 5 (a copy of the Treasures move) and the arrows of the old last move now stay visible.
* **Entering.** X (`T5[1]`) plays the exit animation and root frame 12 dispatches on `ShellMenu_State` (`GotoFrame` 114, 67, 15, 43 for
  states 1..4): a new block sends state 5 to the list page, frame 150. **Returning:** Back / Triangle from the list sets
  `ShellPortReturn` and goes to frame 2 (the title menu appears); a block added to frame 2's script then shows Port Options instead of New
  Game (`GotoLabel 'UpOn'`: the row clip has no `On` label), with the arrows of the last item.
* **Pages.** Root frames 150/151 (the list), 152/153 (Cheats), 154/155 (In Game Overlay), 156/157 (Image Adjustments): an init frame
  with a label and a Stop frame, the layers of a page with entries at both frames and a removal at the first frame of the next page
  (the same technique as in the pause menu). The rows are clones of the Options page of the title menu: the toggle rows of "Progressive
  Scan" (`T6[39]`, label right-aligned to the same column), the slider rows of "Music Volume" (`T6[36]`), "Controller Display" (`T6[24]`) for
  the list items and Reset, "Return to Main Menu" (`T6[20]`) for Back, and the banner `T6[42]`. The entries copied are the ones at frame 20
  of the Options page, its resting state (the earlier ones are the pop-in animation, with other color multipliers: the first try copied
  frames 15 and 16 and the banner and the hint came out dim).
* **Key handlers.** One new `T5` record per key (X, Up, Down, Right, Left, Triangle: clones of the Options page's `T5[5..10]`), each
  entry just `Call`ing a subroutine root frame (`PortKeyX`, `PortKeyUp`, ... frames 158..163, like the title menu's own handlers call
  `OpeningUp`), placed over frames 150..157. The subroutines dispatch on `ShellPort_Page` (0 list, 1 Cheats, 2 Overlay, 3 Image) and
  `ShellPort_Row`; they start with the three guards of the title menu (`PS2_EnableButtons`, `DiscRemoved_DisableButtons`,
  `EnableButton`). Keeping the scripts out of the T5 entries matters: with the scripts inline (each key entry holds its own copy, two
  entries per direction) the movie was 42 KB bigger.
* **Memory.** The patched movie (+ tables, 222,980 bytes) is larger than the 166,972-byte buffer the game loaded the retail one into. The
  hook (`gowNativeMenuBuildMovie`) therefore allocates a block of that size from the game's heap (`0x13DC78`) and calls the original
  `BuildMovie` with the block as its data argument: the loader parses in place there (the movie object it returns is what the caller
  keeps), and the retail buffer stays unused. Net cost: about 223 KB of heap for the whole session, on top of the 143 KB of the pause menu
  patch. The Shell patch is only applied after the pause menu patch took effect (the pages need its variables).
* **Not changed:** Options and Treasures of the title menu (checked on screen), Load, the new game flow (a new game started from the
  patched title menu reaches the first zone and the pause pages work there).

## 6. Extension technique (installed)

The extension point is the movie build: `Flash_BuildMovie` (`0x14AE68`) is replaced by `gowNativeMenuBuildMovie`
(`gow_menu_diag.cpp`, installed by `gow_overrides.cpp` when `GOW_NATIVE_CHEATS_MENU=1`).

1. **When.** `R_PERM.WAD` is built at boot, long before the HUD object exists. For the retail HUD movie the resource has flags
   `0x2008` (bit `0x2000`: parsed **in place**, in the buffer that holds the file). The hook recognizes it by size (479,303), the
   in-place flag and the counts T0 936, T1 220, T3 199, T6 381, T9 1496.
2. **What is written over the buffer.** The patched movie, built by applying an embedded **delta**
   (`ps2xRuntime/src/game/gow_menu_patch.h`) to the movie in memory: a list of *copy(offset, length)* and *literal* operations
   whose output is the movie (386,442 bytes) followed by its T8 table (130,840 bytes). The header lists the size and CRC32 of the
   retail movie and of the result; the runtime checks both and otherwise leaves the original menu untouched (`FLP_HUD is not the
   retail movie ...`). The repository therefore holds no game data: copies only refer to offsets, literals are the new records
   (about 25 KB for the three pages, their rows and the handlers) and the fields they changed. The Definitive Edition movie has another
   CRC and is never patched.
3. **The volatile window.** The game itself writes heap bookkeeping into one table of the loaded movie (records
   `ff ff NN 00 96 00 xx 0b 00 00`, around `0x1FF4C`; 23 bytes differ between the file and memory, stable between runs). Both
   checksums therefore ignore the window `0x1FF30..0x20000`, and bytes copied from it keep whatever the game has there, as for
   the original menu. (Without this the first runs refused to patch: CRC `FA44B346` in memory against `963331D1` of the file.)
4. **Memory budget.** The in-place buffer is 479,303 bytes and the heap has little slack in that zone: a second copy of the 480 KB
   movie broke zone loading (a null `JALR` after heap exhaustion, reproduced with an identity movie padded to 481,215 bytes), while a
   few hundred KB of extra heap are fine (probe with an unused block: 150 KB and 300 KB reached the Options menu without problems).
   Each cheat row costs about 2.3 KB in the movie (a cloned row clip with its 95 placement entries, a label, a toggle subroutine, a
   pose in the page init and its share of the handlers), so nine rows (+18.7 KB) did **not** fit the buffer (491,202 bytes) even
   without T9. Pagination would not help (the cost is per row, not per screen). Solution: the two **read-only tables** are left out
   of the movie and live in blocks of their own: **T9** (1496 color multipliers of 8 bytes, 11,968 bytes) and **T8** (6,542
   transforms of 20 bytes, 130,840 bytes, edited and extended by the patch). Both blocks come from `Heap_Alloc` `0x13DC78` (heap
   from `0x13E090`, 16-byte aligned, as the game reads some tables with 128-bit loads) and add about 143 KB of heap in total (the overlay and image pages added 260 bytes of T8).
   The movie shrinks to **386,442 bytes** (it was 362,890 with the Cheats page only). The header counts (`+0x48` T8, `+0x50` T9) are 0 while the loader walks the movie, and
   the pointers (`+0x4C`, `+0x54`) and counts are written by `finishMovie` later.
5. **Checkpoint resumption (important).** The loader walk is a long guest function that an EE checkpoint can interrupt and
   resume later: right after `BuildMovie` returns, the header pointers of T7, T8 and T10 are still stale. Writing the T9 fields
   immediately made the HUD render yellow and white (several wrong diagnoses before the cause: alignment, shifts, counts).
   Rule: after calling the original, write nothing; wait until `*(data + 0x5C)` (the T10 pointer) points inside the buffer.
6. **The variables.** `prepareExtension` (first `Hud_LoadMovie` with the HUD object, `validateHud` 8/8 and fewer than `0xBF`
   variables; 170 + 17 are used) calls `Hud_RegisterVariable(hud, type 7, name, flag 0, storage, default 0)` for each of the nine cheats
   and the four overlay lines. The movie sees `PS2_GodMode`, `PS2_OneHitKill`, `PS2_InfiniteMagic`, `PS2_InfiniteRage`, `PS2_MaxOrbs`,
   `PS2_AllUpgrades`, `PS2_AllPowers`, `PS2_MaxMeters`, `PS2_UnlockExtras`, `PS2_OsdFps`, `PS2_OsdGpu`, `PS2_OsdCpu`, `PS2_OsdRam`,
   `PS2_ImgBrightness`, `PS2_ImgContrast`, `PS2_ImgGamma`, `PS2_ImgSaturation` (the last four hold the percentage 0..100, the others 0/1).
   Words and names live in a 384-byte block taken from the game heap together with the T8/T9 blocks (the earlier scratch words at
   `0x7F10` were dropped: seventeen names need more than that spot is known to hold; the HUD table has room for `0xBF`).
   **The flag argument must be 0**: `Hud_SetVariable` ignores writes from the movie when the entry's `+0xC` is not zero (the first
   attempt used 1 and the row never changed). No block means the patch was not applied: nothing is registered.
7. **Sync.** `gowNativeMenuVBlank` (registered with `gowSetVBlankExtra`, called after `gowCheatsVBlank` on the EE thread) compares,
   for each variable, the guest word with its engine: if the movie changed the word it calls `gowCheatsSet(i, ...)` (cheats) or
   `gowOverlaySet(i - 9, ...)` (overlay lines) or `gowImageSet(i - 13, percent)` (sliders), if the engine changed (F2 menu, `PC_CHEATS`) it writes the word. Whoever changed last
   wins.

Other rules kept: validate before touching anything, delegate to the original when `ctx->pc` differs from the entry, refuse
to install if any check fails, nothing per frame except the one comparison.

## 7. Guest/host interoperability

Overrides follow `gow_overrides.cpp`: replace by address, call the generated function, and when `ctx->pc` differs from the
entry call it immediately. Guest strings are read with a bounded copy (`guestBuffer`). Calling guest functions from an override
(`callGuest`) saves and restores the whole context. The extension writes the guest in three places only: the patched movie
(over its own buffer), the T8 and T9 blocks it allocated, and the variable block it allocated (words and names).

## 8. Cheat integration

`RustPortPS2/native/ps2xRuntime/src/lib/gow_cheats.h` declares a small API that `gow_cheats.cpp` implements on the same
atomic state the F2 menu uses (`g_active`), by cheat index in the order of the F2 list (`god_mode`, `one_hit_kill`,
`infinite_magic`, `infinite_rage`, `max_orbs`, `all_upgrades`, `all_powers`, `max_meters`, `unlock_extras`):

* `gowCheatsAvailable()`: true only after `gowCheatsInstall` succeeded.
* `gowCheatsCount()`, `gowCheatsGet(index)`, `gowCheatsSet(index, bool)` (refuse when unavailable or out of range).

The effects are not duplicated: each cheat is still the F2 menu's own (the health wrappers `0x1E0A80`, `0x1E0A98` and the VBlank
refill for God Mode, the other values applied while active). **Hardcore** (`PC_RA_ENABLED=1` and `PC_RA_HARDCORE=1`) makes
`gowCheatsInstall` return before setting `g_installed`: `gowCheatsAvailable()` is false, the movie is **not** replaced (the player
sees the stock pause menu, no Port Options row) and the setters refuse anyway.

### Overlay integration

`RustPortPS2/native/ps2xRuntime/src/lib/gow_overlay.h` declares `gowOverlayCount()` (4), `gowOverlayGet(index)` and
`gowOverlaySet(index, bool)` for the parts of the performance overlay (`gow_osd.inl`, drawn by `gow_integration.cpp`) in the order
FPS (0), GPU (1), CPU (2), RAM (3). The state is one atomic bit mask (`g_osdMask`): the launcher's `PC_SHOW_FPS`, `PC_SHOW_GPU`,
`PC_SHOW_CPU`, `PC_SHOW_RAM` give its start value, the menu changes it from the EE thread and the render thread reads it. A change is
written back for the next session ("Persistence" below). Differences from the launcher-only OSD that matter:

* The game-frame hook (entries to `0x1837B8`, the source of the FPS number) used to be installed only when some part was on at start.
  With `GOW_NATIVE_CHEATS_MENU=1` it is always installed (one relaxed atomic increment per game frame), so the FPS counter can be
  switched on in the middle of a session.
* When the overlay goes from hidden to shown the measuring window restarts, and the FPS line waits for its first full second instead
  of drawing a 0 or an average over the whole time it was off.
* The sampler thread (PDH counters, D3DKMT) starts the first time a GPU, CPU or RAM line is drawn and keeps running afterwards; what the
  system cannot report is not drawn (never a made-up value), as before.
* Hardcore: the menu extension is off as a whole (it hangs on `gowCheatsAvailable()`), so the page is not offered; the launcher's
  switches still work.

### Image integration

`RustPortPS2/native/ps2xRuntime/src/lib/gow_image.h` declares `gowImageCount()` (4), `gowImageGet(index)` (percent 0..100, 50 = no
change) and `gowImageSet(index, percent)` for the Shade Boost of `gs/gs_host_present.cpp`, in the order brightness (0), contrast (1),
gamma (2), saturation (3). The four values are atomics (`g_shadeLevel`): the launcher's `PC_BRIGHTNESS`, `PC_CONTRAST`, `PC_GAMMA` and
`PC_SATURATION` give their start value (read once, 50 when absent), the menu changes them from the EE thread, and the render thread
turns them into the shader's uniforms every presented frame (`shadeBoost()` is now computed per frame instead of cached). A change is
written back for the next session ("Persistence" below). What changed for the renderer:

* The scaler shader is always loaded with `GOW_NATIVE_CHEATS_MENU=1` (it used to be loaded only for the sharp or FSR scalers, FXAA or a
  non-neutral Shade Boost at start). With `GOW_SCALER=bilinear` it keeps its one linear tap (`plain`), so the picture is the same until a
  setting is changed.
* The adjustment applies to the final picture, menus included, so the page previews itself while you move a slider.
* Reset to Default means the neutral value (50 for each), not the launcher's start values. A start value of the launcher (any 0..100) is
  kept exactly: the variable is the percentage, the bar shows the position of value / 10, and the first press moves it by the step.
* Hardcore: the whole extension is off, so the page is not offered (the launcher's values still apply).

### Frame Rate integration (FPS limit, VSync, Early FINISH)

`lib/gow_frame.h` (`gowFrameGet/Set/Available`, three indexes: 0 FPS limit as 0, 1 = 60, 120 (it had a third value, 240, until the project capped every rate at 120); 1 VSync;
2 Early FINISH) is the
interface of the menus' fourth item, **Frame Rate**: three HUD variables (`PS2_FpsLimit`, `PS2_VSync`, `PS2_EarlyFinish`; the variable
block grew to 416 bytes and the table to 190 of 191 entries). Each setting used to be read once at start-up, so making them change in
game meant making every reader dynamic:

* **FPS limit** (the logic rate, `PC_LOGIC_FPS`): `gowLogicFps()` is an atomic now (`gowFrameSet` writes it). Its readers stopped caching
  it: the pacing points per field of the EE scheduler (`EeScheduler.cpp`, `paceSteps()`), the window's latch on each game swap
  (`gs_host_present.cpp`, `latchOnSwap`), the frame counter's limit, and the minimum time between two new window frames (`gs_gl.cpp`,
  `gowPresentMinIntervalUs()`: the launcher sets `GOW_PRESENT_MIN_INTERVAL_US` to 800000 / fps, and when the environment holds exactly that
  value it now follows the limit chosen in game; a value set by hand is kept). The two overrides of `gow_logic_rate.cpp` (frame end wait
  `0x17A1D0`, time step `0x17D778`) are bound whenever the menu extension is on, and the frame wait runs the original (the VBlank count)
  when the limit is 60. Not available with the game's 30 FPS mode.
* **VSync**: `gowVsyncEnabled()` (atomic) replaces the read of `PC_VSYNC` in `hostSyncToVBlank()`; the window loop (`gowFrameInput`, render
  thread, the one that owns the GL context) applies a change with `SetWindowState` / `ClearWindowState(FLAG_VSYNC_HINT)` and resets
  `SetTargetFPS` to the pacing the new mode needs (with VSync the display paces the loop, without it the game's VBlank does).
* **Early FINISH**: `PS2Memory::m_gsEarlyFinish` is an `std::atomic<bool>` (the header `ps2_memory.h` changed: all generated units were
  recompiled once) and `setGsEarlyFinish()` first drains the GS worker (`waitGsWorkerIdle`) so that no job queued under the old rule is
  left with a FINISH nobody raises (a job queued with early FINISH has its own FINISH writes skipped), then flips the flag. EE thread only;
  without the GS worker (`PS2X_GS_WORKER`) there is nothing to switch.
* **Menus.** The FPS limit row is a clone of the Sound Mode row (`clone_selectN` in `flp_patch_cheats.py`: its label plus one clip per value,
  with the texts "60 FPS" and "120 FPS" in the style of Mono / Stereo / Surround; a third value, "240 FPS", was removed with the
  120 FPS cap); Left / Right / X cycle through the values like the
  stock row does. VSync and Early FINISH are toggle rows. In the pause menu the page is root-clip frames 32 and 33 (the Image page's layers
  are now removed at 32), `PausePort_State` 4 is the list row and 9 the open page, `PauseFrame_State` the sub-state. In the title menu it is
  root frames 158 and 159 (`ShellPort_Page` 4); its select row also shows and hides the arrows inside the three value clips, as
  `OptionsDown` does (`highlight()` in `flp_patch_shell.py`). Persistence adds `fpsLimit`, `vsync` and `earlyFinish` to the JSON.

### The variable bank and the Image Quality page

**Why a bank.** The movies reach the settings through HUD variables (`PS2_<name>`, registered in the HUD object's table). That table is
an open-addressing hash of **191 entries** (`0x1B5C70`: start index from the name's hash, `0xBF` slots) and the game registers 170 of
them: one variable per switch (9 cheats, 4 overlay, 4 image, 3 frame rate = 20) left one free slot, and the Image Quality page needed
five. Entries cannot be removed (a removal breaks the probe chain of the names that collided with it), and the 20 registered names
that neither movie mentions (`EnemyMeter_*`, `Zone_Choice`...) may belong to the level movies, so none was reused. The settings
therefore share **ten variables**: `PS2_PortPage` (which page is open) and `PS2_Slot0..PS2_Slot8` (that page's values, row by row; the
longest page, Cheats, has nine). `gow_menu_diag.cpp` keeps them: on a change of `PortPage` it refills the slots from the new page's
current settings (nothing of the old page's values is carried over), and every VBlank it copies a slot the movie changed into its
setting, and a setting that changed elsewhere (F2, the other menu) into its slot. The movie arms a page **while its row of the Port
Options list is highlighted** (`arm()` in the generators: on each highlight move, in the list's init, and when a page opens), so the
slots already hold the right values when the page opens and its init can read them; nothing relies on a frame passing in between.

**Image Quality** (`lib/gow_quality.h`, four settings: render resolution 0..7 = 1x..8x, upscaling 0 bicubic / 1 bilinear / 2 pixel,
sharpness 0..100, FXAA), all live:

* *Upscaling, sharpness, FXAA* (`gs_host_present.cpp`): `g_filterMode`, `g_sharpen` (0..0.5, the shader's strength; the menu shows it as
  a percentage of that) and `g_fxaaOn` are atomics read every frame (they were `static const` reads of `GOW_SCALER`, `GOW_SHARPEN` and
  `PC_FXAA`). The scaler shader is already always loaded with the native menu. Pixel is drawn like bilinear (the launcher sends both as the
  bilinear stretch) but is remembered as its own value.
* *Render resolution* (`gs_hw_backend.cpp`): `gowGsScaleSet` records the request and `applyPendingScale()` runs at the start of the next
  draw, clear or present, on the thread that owns the GL context: it flushes the pending batch, re-creates every render target at the
  new size (colour, and depth when present), copies the picture with a framebuffer blit (linear for colour, nearest for depth), deletes the old
  ones and changes `scale`; everything else sizes by `scale` at the moment it is used. The maximum is the GPU's texture limit. The
  hardware backend never hands the picture out on the CPU, so no readback cache depends on the old size.
* *Progressive scan (480p) is not in the menu.* It was, first live and then as a "next start" row. A live switch makes the game's own
  mode check change the video mode, the GS is reset and the models lose their textures; a row that saves a value and does nothing
  visible until the next start is confusing, so the setting stays a launcher setting (`GOW_PROGRESSIVE`, `gow_video.cpp` is the
  retail behaviour again) and the game does not write `progressive480p` to the state file.
* The variables are `PS2_Slot0..3` in the page; the select rows (resolution has eight values, upscaling and the FPS limit three) are
  clones of the Sound Mode row built by `clone_selectN` (the label, plus one clip per value with its own text and instance name, in
  the style of the original first value), the sharpness slider is a clone of the Music Volume row stepping 5 %, FXAA is a toggle row.
  In the pause menu the page is root-clip frames 32 and 33 (Frame Rate moved to 34 and 35); in the title menu it is root frames 158 and
  159 (Frame Rate 160 and 161). Row labels must fit the column ("Progressive (restart):" overran the value). Persistence adds
  `internalResolution`, `filter`, `sharpness` and `fxaa`.

**The volatile window of the pause movie (a trap, found while removing the Progressive row).** The pause movie is parsed in place in the
buffer the game loaded the retail one into, and the game's heap writes block pointers (for example `c8 55 30 00` and `40 99 30 00`)
into `[0x1FF30, 0x20000)` of that buffer. In the retail movie that range holds dead records. A rebuilt movie has whatever its layout
puts there, so every change of the generator moved live records under the writes: some sizes worked and others did not, with no
relation to what had changed. Seen: a title menu that stayed black (a label of different length), and a stack overflow of the host
(exit code `0xC00000FD`, a long repeating chain `0x146de8 -> 0x149958 -> 0x147520`, then a `missing-target` JALR from `0x149b30` on a
zeroed object) as soon as the title movie was built after the pause movie (removing one row). `flp_model.serialize(..., window=...)` now
makes the window dead padding: zero bytes after the End action of the last frame action blob that ends before it (the interpreter never
reaches them), 1.2 KB more in the movie, and checks that the window is inside it. `GOW_MENU_WINDOW_DUMP=1` (with `GOW_MENU_DIAG=1`) prints
the window as the game left it after the load. The title movie has its own block and no window.

### Persistence (what the player changes in game goes back to the launcher)

Every change of a cheat (`setActive`, so F2 and the pages alike), of an overlay line (`gowOverlaySet`) and of an image adjustment
(`gowImageSet`) calls `gowPersistTouch()` (`gow_persist.h`, defined in `gow_integration.cpp`) when the value really changed. With
`GOW_INGAME_STATE` set (the launcher sets it to `ingame-state.json` in its data folder), a thread writes that file once the changes
have been quiet for 0.4 s (a slider held down changes ten times a second) and `atexit` writes it a last time: one JSON object with
`cheats` (the ids of the active ones, only when cheats are available, so never under Hardcore), `showFps/Gpu/Cpu/Ram` and
`brightness/contrast/gamma/saturation`. The launcher (`gow-core/src/ingame.rs`) applies it to its settings when the game ends and
before the next start, validating each field (types, 0..100, known cheat ids), then deletes it. A killed game loses at most the last
0.4 s of changes. Not written without the variable, so tests and games started by hand leave the settings alone.

## 9. Diagnostics and feature flag

* `GOW_NATIVE_CHEATS_MENU=1` installs Port Options with its Cheats, In Game Overlay and Image Adjustments pages (opt-in; the launcher always sets it; implies the observers below). Log tag `[gow-menu]`.
* `GOW_MENU_DIAG=1` registers three observers that always call the original: `0x1B8A48` (message bank counts), `0x1B9358` (prompt
  ids, at most 64 samples), `0x1B7CE0` (movie names, at most 64 samples), and `validateHud` on the first movie load.
* Lines to look for: `FLP_HUD replaced in place by the patched movie (479303 -> 386442 bytes; T8 130840 bytes at ..., T9 at ...)`,
  `Port Options pages: 17 variables PS2_<name> registered (9 cheats, 4 overlay parts)`, `patched movie complete: T8 (6542 entries) at ..., T9 (1496 entries) at ...`,
  `GodMode changed from the Port Options menu: ON` (and the same for the other cheats and for `OsdFps`, `OsdGpu`, `OsdCpu`, `OsdRam`, ON or OFF; `ImgGamma changed from the Port Options menu: 70%` for the sliders), and the refusals
  `extension off: cheats are unavailable ...` and `FLP_HUD is not the retail movie ...`.

## 10. Validation evidence

Setup of every run: retail data in `D:\GOW_TESTE\game` (the launcher's install), the launcher environment (`cargo run -p gow-cli -- env`),
internal scale 4, a **copy** of the memory card (`GOW_MC0`), the FastDev build (`build-dev`, logs ON), scripted pad
(`GOW_PAD_TEST=1`, `GOW_PAD_SCRIPT` with gameplay-relative times `g<seconds>`), window captures with `GOW_HOST_SHOT`.

| Check | Result |
|---|---|
| Original menu without the flag | Stock pause menu (four rows) and Options page (nine rows), unchanged. |
| Options page with the flag | Unchanged from the original (nine rows, Return to Pause Menu), no Cheats row. |
| Pause menu | Five rows (Continue, Options, **Port Options**, Restart, Quit Game), evenly spaced, "Port Options" centered; Down from Options stops at Port Options, Down again reaches Restart, Up from Restart returns to Port Options. |
| Port Options list | Banner "PORT OPTIONS" (fits the plate), "Cheats" highlighted on the first row, "Back" on the last; Down moves to Back; Back via X returns to the pause menu with Continue highlighted. |
| Cheats page | Banner "CHEATS", nine value rows (God Mode, One Hit Kills, Infinite Magic, Infinite Rage, Max Orbs, All Upgrades, All Powers, Max Meters, Unlock Extras) with names ending on one line and Off/On in a column, "Back" in the last row; opens from the list with X; the layers of the list are gone and the layers of the page do not leak under the list. |
| Navigation and toggling in the page | Down moves row by row (highlight animation as in the original); three cheats toggled in one run (God Mode, One Hit Kills, Infinite Rage), each logged as `[gow-menu] <Name> changed from the Port Options menu: ON` with the row showing On; X on Back returns to the Port Options list; with `PC_CHEATS=max_orbs,all_powers,unlock_extras` the page opens with exactly those three rows On. |
| Port Options list with two items | Banner "PORT OPTIONS", "Cheats" and "In Game Overlay" on the first two rows, "Back" on the last; Down moves Cheats, In Game Overlay, Back. Coming back from the In Game Overlay page the list highlights "In Game Overlay" and none of the page's layers remain (Release build, repo `build/`, scripted pad, `PC_SHOW_*` all 0). |
| In Game Overlay page | Banner "IN GAME OVERLAY" (font scale 620, inside the plate), rows "FPS Counter:", "GPU Stats:", "CPU Stats:", "RAM Usage:" (acronyms in capitals, names ending on one line, Off/On in a column) and "Back" right below. With every line off at start, X on FPS Counter and on GPU Stats logged `[gow-menu] OsdFps changed from the Port Options menu: ON` and `OsdGpu ... ON`, and the corner showed the GPU, MEM and FPS lines at once (CPU and RAM stayed hidden); the FPS value appeared (60) after its first second. With every line on at start (launcher defaults) the page opened with four rows On and the corner showing all lines; X on FPS Counter logged `OsdFps ... OFF`, the row showed Off and the FPS line disappeared while GPU, MEM, CPU and RAM stayed. |
| Cheats page behind the new list | Still opens from the list's first item; a toggle (God Mode) logged `GodMode changed from the Port Options menu: ON`; nine Down steps and X on Back returned to the list with "Cheats" highlighted. No `missing-target` in any of the four runs. |
| Port Options list with three items | Banner "PORT OPTIONS", "Cheats", "In Game Overlay", "Image Adjustments" on the first three rows and "Back" on the last; after Back from the Image Adjustments page the list highlights "Image Adjustments" (Release build, repo `build/`, scripted pad). |
| Image Adjustments page | Banner "IMAGE ADJUSTMENTS" (font scale 540, inside the plate); four slider rows (Brightness, Contrast, Gamma, Saturation) with the game's own bar, then a free row, "Reset to Default" and "Back"; the block is centered once `IMAGE_TX` = -400 (it sat 32 px right of the center at 0, measured at 1366 px). First version (level = variable, 10 % per press): too coarse, the player said so. Current version (the variable is the percentage, `IMAGE_STEP` = 2): Right on Brightness six times logged `ImgBrightness changed from the Port Options menu: 52%`, `54%` ... `62%`; Contrast left three times logged 48, 46, 44; Gamma right twice logged 52, 54. The bar poses at value / 10 + 1 and the engine's frame jump **truncates** the fraction: 62 % showed position 6, 44 % position 4, 54 % position 5 (measured on the slider knob). The picture, menu included, followed the values (in the first version saturation 0 made it grey). |
| Reset to Default and limits | With `PC_BRIGHTNESS=99 PC_CONTRAST=1` (start values that are not multiples of the step, logged by `[gs-present] shade boost: brightness 99 contrast 1 ...`): one Right on Brightness logged `ImgBrightness ... 100%` (99 + 2 clamped to 100), one Left on Contrast logged `ImgContrast ... 0%` (1 - 2 clamped to 0); X on "Reset to Default" logged `ImgBrightness ... 50%` and `ImgContrast ... 50%` and every bar went back to the middle, picture normal. No `missing-target` in any run. |
| Triangle | Scripted taps of 0.15 s (Release build, Agile combat on as in the launcher): from the Image page one tap shows the Port Options list with Image Adjustments highlighted; two taps show the pause menu (Continue highlighted); three taps are back in the game; from the In Game Overlay page one tap shows the list with that row highlighted; from the stock Options page one tap shows the pause menu. Before the fix (input buffer of Agile combat active in the pause) the same single tap ended in gameplay in every case, stock Options included. |
| Title menu: the item | Press Start, Down four times: the fifth item "Port Options" appears after Treasures with only the up arrow; Up goes back to Treasures. Retail `FLP_Shell` replaced by the patched movie (`[gow-menu] FLP_Shell replaced by the patched title menu (166972 -> 222980 bytes ...)`). Options and Treasures open as in the original, a new game started from the patched title menu reaches the first zone. |
| Title menu: the pages | X on the item opens the list (Cheats, In Game Overlay, Image Adjustments, Back); Cheats page (X on God Mode logged `GodMode changed ...: ON`), In Game Overlay page (X / Right turned FPS, GPU, CPU on: `OsdFps`, `OsdGpu`, `OsdCpu` ... ON), Image Adjustments page (Brightness right three times 52, 54, 56 %, Contrast left twice 48, 46 %). Triangle on a page returns to the list with that item highlighted; Triangle on the list returns to the title menu on the Port Options item. |
| Title menu: the rest of the pages | Image Adjustments: after Brightness 52, 54, X on "Reset to Default" logged `ImgBrightness ... 50%` and every bar was back in the middle. In Game Overlay: the fourth row (RAM Usage) logged `OsdRam ... ON`. Cheats: Left on God Mode logged ON, Left on One Hit Kills ON and Left again OFF. |
| Zone changes with the patches | `GOW_WARP` through PAND03C, PAND03D, PAND02B, PAND01A, then PAND04G, PAND05A, PAND03E, PAND02H, PAND00A, PAND03J (11 zone changes in two Release sessions, 12 s in each, scripted, the title menu and pause menu patches both active): no crash, no `missing-target`, and the pause pages still worked after the last zone (X on God Mode logged ON; the Image Adjustments page was drawn after four warps). Repeated with the larger title block and the Image Quality page (4 warps, then the Cheats and Quality pages: OneHitKill ON, resolution 5x, state file written; then a fresh session with PAND04G, PAND05A, PAND03E, PAND02H, PAND00A, PAND03J): no crash. Chaining all six zones in one session stopped with a `missing-target` at PAND04G after PAND01A; the two-session split above is the tested route (not checked whether that chain also fails without the patches). |
| Image Quality, pause and title menus | Pause: Resolution 4x to 5x, Upscaling bilinear, Sharpness 95/90/85 %, FXAA OFF, each logged as `[gow-menu] ... changed` and written to the state file (`internalResolution`, `filter`, `sharpness`, `fxaa`); the scene (models, textures) stays intact after changing them. The page has four rows (no Progressive). Title: the same page drawn and driven (resolution 5x, FXAA OFF logged and written to the state file). |
| Holding Left / Right | Before: Right held 2.5 s on a slider gave one step (52 %), the menus act on the press only. Now (`GOW_MENU_REPEAT`): Right held 2.5 s from 50 % logged 52, 54 ... 86 % (the first step at once, then about every 0.12 s after 0.4 s), Left held 1 s 84 ... 76 % (title menu); the same on the pause menu page (Right 1.5 s: ten steps, Left 1 s: five). The repeat is only outside gameplay (game state 11). |
| Frame Rate page | Title menu and pause menu: the page shows FPS Limit (60 / 120 / 240 FPS, with arrows when highlighted), VSync and Early FINISH. Live in game (pause page): Right on the FPS row logged `FPS limit 120`, then 240, then 60; the logic rate went from `logic=60` (60.0 calls/s) to `logic=120` (about 100 calls/s in the heavy test scene, the same as a start with `PC_LOGIC_FPS=120`: 91 to 110 calls/s) and back to `logic=60` at 59.9 to 60.0 calls/s with `waits=0` (the original VBlank pacing). VSync on and off logged `[gow-frame] VSync on/off (changed in game)`, Early FINISH off and on logged `[gs-worker] early FINISH off/on (changed in game)`, in a session that went on to the end of the script with no hang. The title page saved `"fpsLimit":120,"vsync":true,"earlyFinish":false` to the state file. |
| Bilinear scaler | `GOW_SCALER=bilinear` (the shader is loaded only because of the native menu): `PC_BRIGHTNESS=100` against the default gave a mean luma of the gameplay frame of 93.7 against 35.0, and Brightness raised from the pause page to 86 % (hold, `shade boost` start values 50) gave 73.5 after leaving the pause menu: the picture follows the sliders in that mode too. |
| Overlay lines from the page, in game | In the pause menu page: GPU, CPU and RAM switched on, then three Triangle taps back to the game: the GPU / MEM / CPU / RAM lines are drawn on the gameplay frame. |
| Cheats from the page, in game | God Mode and Max Orbs switched on in the pause menu Cheats page (`GodMode ... ON`, `MaxOrbs ... ON`): Kratos, left idle among the enemies, was alive at 164 s (the same scripted idle run, no cheat, ended on the death screen at about 150 s) and the orb counter of the HUD no longer showed 0. A second run with Infinite Magic, All Upgrades, All Powers and Max Meters switched on from the page (four `... changed ... ON` lines): the HUD showed a full, extended blue magic meter and the game raised the Rage of the Gods tutorial prompt that unlocking the powers triggers. |
| Patch from the embedded delta, no external files | Applied on every run: `FLP_HUD replaced in place ... (479303 -> 376286 bytes; T8 ..., T9 ...)`; the header is reproduced byte for byte from the retail PAK by `make_patch.py`. |
| Heap probe | An unused extra block of 150 KB and of 300 KB (temporary probe, removed): the game still reached gameplay and the Options menu; the patch needs about 143 KB. |
| Wrong movie | Before the volatile window was masked the checks refused (CRC `FA44B346`) and the stock menu stayed: the guard works. |
| Hardcore (`PC_RA_ENABLED=1 PC_RA_HARDCORE=1`) | `[cheats] RetroAchievements Hardcore active; cheat menu disabled`, `[gow-menu] extension off ...`, the stock four-row pause menu without Port Options. |
| Release build (`RustPortPS2/native/build`: Release, logs OFF, no console, unity batch 8, AOT VU1 from 413,901 captures plus `vu1n_heads.txt`, clang from `toolchain/`; 1,053 steps, `[generate_vu1] ... 4096 images`) | The Cheats page works in it (God Mode and One Hit Kills toggled, navigation as in the FastDev build). Launcher environment (`GOW_PERF_DIAG=1`, `PS2X_GS_WORKER=1`, `[gs-worker] VIF1/GIF processing on a worker thread` in the log), no observers, scripted combat inputs: 60 fps steady (300 frames per 5 s), worst frame 17.5 to 18.2 ms, no frame over 25 ms during gameplay, with the extension off and on alike (the only hitch is the 818 ms boot load, in both). |
| Stability | No `missing-target` or crash in any run above; HUD colors correct after the checkpoint-resume fix. |

**Not verified**: any of the pages with a physical controller (every run feeds the pad through the scripted presses, which reach the game as the same pad state; the held-direction repeat was measured that way), the slider sound (no audio check); the overlay switched on mid-game in a very long session (the sampler thread keeps running once started; 150 s were run); the Release performance run is a 70 s window in the first zone with scripted attacks, not a comparison against a build of the previous commit (the extension off and on in the same build behave alike; speed-profile PGO was not trained); the in-game effect of One Hit Kills, Infinite Rage and Unlock Extras through this menu (the engines are the already existing F2 cheats, only the front end is new; pre-existing behaviour of `unlock_extras` persists on the memory card once you save); Triangle with a real controller (scripted presses only); a second
language (the labels are glyph content, the new words are English only); the Definitive Edition data (never patched); Linux.

## 11. Limitations and hypotheses

* The patch is tied to the retail USA `FLP_HUD` (CRC and size in `gow_menu_patch.h`); anything else keeps the original menu.
* The text is English. The pt-BR launcher strings do not apply to glyph content of the movie.
* The centering offset (340 twips) was measured on screen at 1366x768, not derived from the renderer.
* The highlighted row grows to the right of the column (the highlight animation scales the row about its origin), like the highlighted "Progressive Scan" row of the Options page.
* The meaning of the `a` and `b` placement fields beyond "indexes into `T8` and `T9`" and of the second draw pass of a text run
  (probably a shadow) is PROBABLE, not proven.
* `Hud_PumpEvents`, `Flash_ExecuteActions`, `Flash_FSCommand`, `Asset_Find` are PROBABLE; `Hud_FlagMovies`, `Hud_PopEvent`,
  `Flash_ParseMovie` are HYPOTHESIS.

## 12. Regenerating the patch and tools

`python RustPortPS2/native/tools/menu/make_patch.py <PART1.PAK>` extracts the movie from the user's own game, builds the patched
movie and rewrites `gow_menu_patch.h` (see the README of that folder). Work files stay in the git-ignored `build-gen/menu-patch/`.
Local research tools (git-ignored, `build-gen/menu-research/`): `flp.py` (parser that reproduces the loader cursor), `flp_clip.py`,
`flp_sprites.py`, `flp_actions.py`, `wad.py`, `gowre.py` and friends, plus the test launchers `run_test.ps1` and `shot_variant.ps1`.
To test by hand (new game, pause, Options, down to Cheats, open the page):

```
GOW_NATIVE_CHEATS_MENU=1 GOW_MENU_DIAG=1 GOW_PAD_TEST=1 \
GOW_PAD_SCRIPT="5:start,12:x,20:x,28:x,36:x,52:x,60:x,68:x,76:x,84:x,100:x,116:x,132:x,g12:select,g16:down,g19:x,g23:down,g25:down,g27:down,g29:down,g31:down,g33:down,g35:down,g37:down,g41:x" \
GOW_HOST_SHOT=9700 ./run_gow.sh
```

Warn the user first and never launch while a `gow-2005` is running.

## 13. Reusable extension points

* `Flash_BuildMovie` (`0x14AE68`) with the delta mechanism: any other movie edit (new rows, new pages) can ship as a delta.
* `Hud_RegisterVariable` table: any named value the movies read can be added with its own storage and flag 0 (`0xBF` capacity,
  170 + 13 used at run time).
* `Messages_Load` end: the message bank is in guest RAM, mutable after load (append with sorted ids).
* `Hud_ShowPrompt`: original list prompt, up to five choices (not used by the Cheats page, which is movie content).
