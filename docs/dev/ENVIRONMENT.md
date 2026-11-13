# Environment variables

The runtime is configured through environment variables. **Everything here is off by default**: an absent variable
means the shipped behavior. The launcher sets the ones a player needs; the rest exist for development and testing.

Conventions:

- A variable ending in `_DIAG` writes a `[...]`-tagged line to the log (stdout/stderr, i.e. `logs/last.log`).
- A variable ending in `_CAPTURE`, `_DUMP`, `_RECORD` writes a file. **Those files contain game data** (RAM, VRAM,
  microcode) and must never be committed, attached or uploaded.
- Times are in seconds unless the name says otherwise.

## Set by the launcher

The launcher keeps your environment and adds or removes these. Set them yourself to reproduce a player's session
manually:

```
PS2X_GS_WORKER=1 GOW_GS_FINISH_ASYNC=1 GOW_AUDIO_DEVICE=default
GOW_PERF_DIAG=1 GOW_PERF_FRAME_PC=0x001837B8 GOW_NATIVE_CHEATS_MENU=1
```

(`GOW_NATIVE_CHEATS_MENU=0` in your environment turns the Port Options pause-menu entry off: a variable you already set is respected.)

plus, from the settings: `PC_SCREEN`, `PC_WIDTH`, `PC_HEIGHT`, `PC_TITLE`, `PC_PANEL=0`, `PC_ICON`, `PC_ASPECT`,
`PC_WIDESCREEN`, `PC_FILTER`, `GOW_SHARPEN`, `PC_FXAA`, `GOW_PROGRESSIVE`, `GOW_SCALER`,
`PS2X_GS_HW_SCALE`, `PC_GAME_FPS=60`, `PC_WINDOW_FPS`, `PC_VSYNC`, `PC_SHOW_FPS`, `PC_SHOW_GPU`, `PC_SHOW_CPU`, `PC_SHOW_RAM`, `GOW_SKIP_FMV`,
`GOW_FAST_BOOT`, `PC_DMC5_CONTROLS`, `PC_PAD_FACE_MAP`, `PC_PAD_MAP`, `GOW_PAD_DEVICE`, `GOW_PAD_LIST_FILE`, `GOW_PAD_SDL_MAPPING`, `PC_KEYS`, `PC_CHEATS`, `PC_CHEATS_SHORTCUT`,
`PC_LANGUAGE`, `PC_ESC_QUITS`, `GOW_CANCEL`, `GOW_ISO`, `GOW_VU1_CAPTURE`, `PS2X_GS_SHADER_CACHE_DIR`, and the
`PC_RA_*` group when RetroAchievements is enabled.

Note: the launcher also exports `PC_SHARPNESS` and `PS2X_GS_SCALE`, but the runtime does not read them. Sharpening
arrives as `GOW_SHARPEN` and the render scale as `PS2X_GS_HW_SCALE`.

## Presentation

| Variable | Effect |
|---|---|
| `PC_SCREEN=fullscreen\|window`, `PC_WIDTH`, `PC_HEIGHT` | Initial mode and window size |
| `PC_ASPECT=auto\|stretch\|4:3\|16:9\|16:10\|21:9\|32:9` | Shape of the presented picture; `stretch` fills the window |
| `PC_WIDESCREEN=<aspect>` | Hor+ widescreen patch at that aspect, `4:3 < aspect <= 4.0` (so `16:9`, `16:10|21:9|32:9``, `21:9` and `32:9`; `4:3` or anything wider, such as `48:9`, leaves it off). Absent = the game's 4:3 |
| `PC_FILTER=fsr\|bilinear\|pixel`, `PC_FXAA=1` | Filter *intent*; the runtime itself reads `GOW_SCALER` and `GOW_SHARPEN` |
| `GOW_SCALER=sharp\|bilinear`, `GOW_SHARPEN` (0–1) | The scaler that actually runs: bicubic + contrast-adaptive sharpening, or a linear tap |
| `PS2X_GS_HW_SCALE=<n>` | Internal render scale (float, 0.25 to the GPU's limit; below 1 the game is drawn smaller than the PS2 and stretched). This is the one the runtime reads |
| `PS2X_GS_DIRECT_PRESENT=0` | Forces the RAM (readback + upload) presentation; shared GL textures are the default |
| `PS2X_GS_FAIL_TEST=context\|shaders` | Test only: `GSHwBackend::ensure()` fails on purpose before touching GL, to see the on-screen renderer-failure message (English, or Portuguese when `PC_LANGUAGE` starts with `pt`); `GOW_HOST_SHOT=<frame>` writes `host_shot.png` |
| `PS2X_GS_GL_MAX_MINOR=<3..6>` | Test only: the GS context ladder starts at OpenGL 4.<n> instead of 4.6 (then falls down to 4.3), to exercise the 4.3 path on a newer GPU. `[gs-gl]` in the log reports the version obtained |
| `GOW_PRESENT_BLOCKING`, `GOW_PRESENT_FIFO`, `GOW_PRESENT_MIN_INTERVAL_US` | Presentation pacing |
| `PC_GAME_FPS`, `PC_WINDOW_FPS`, `PC_VSYNC`, `PC_SHOW_FPS`, `PC_SHOW_GPU`, `PC_SHOW_CPU`, `PC_SHOW_RAM` | Limits, vsync, overlay lines (`PC_WINDOW_FPS`, set by the launcher: 60 with the 60 limit, so the window follows the game's VBlank, the display's refresh rate with the 120 one; 30–120: the project never runs above 120 and 0 no longer means unlimited). The four `PC_SHOW_*` only set the state at start: with `GOW_NATIVE_CHEATS_MENU=1` the pause menu's **In Game Overlay** page flips each line live (`gow_overlay.h`), and the frame counter hook is then always installed |
| `PC_LOGIC_FPS=120` | Game logic rate (the only one above 60; a higher value gets 120, the project's cap) (`src/game/gow_logic_rate.cpp`): the frame end waits for 1/N s slots of guest time instead of 60 Hz VBlanks, the time step floor becomes 60/N fields, and the window latches a new picture on each game buffer swap. Unset or 60 = the game's own 60 Hz. The launcher's FPS limit sets it (120). Ignored with `PC_GAME_FPS=30` |
| `GOW_HFPS_DIAG=1` | `[gow-hfps]` every 5 s: time step (ticks, fields, delta), fixed-step frames, N Hz waits, late frames and 60 Hz fallbacks, `game_s/host_s` |
| `PC_PROGRESSIVE` / `GOW_PROGRESSIVE` | 480p progressive mode |
| `PC_TITLE`, `PC_ICON`, `PC_PANEL=0` | Window title, icon, hidden developer panel (F1 opens it) |
| `PC_ESC_QUITS=1\|0` | Escape closes the game |

## Threads and pacing

| Variable | Effect |
|---|---|
| `PS2X_GS_WORKER=1` | VIF1/VU1/GIF on the GS worker thread. **Required for real-time audio and speed** |
| `GOW_GS_FINISH_ASYNC=1` | GS FINISH does not block the EE |
| `GOW_FX_SHADOWS=0`, `GOW_FX_ADDITIVE=0`, `GOW_FX_FILTER=0` | Effect switches for weak GPUs (Performance tab): no character shadow volumes, no additive 3D effects (fire, glows, particles), nearest texture sampling. On by default; see [`RENDERING.md`](RENDERING.md#7-internal-scale-below-1x-and-effect-switches) |
| `GOW_GS_EARLY_FINISH=0` | Turns off early FINISH (on by default with the worker): the frame's FINISH is then given when the GS reaches it ([`RENDERING.md`](RENDERING.md)) |
| `GOW_STALL_DIAG=1` | While a game frame is more than 40 ms late, samples the main thread's live pc/ra; one `[gow-stall]` line per late frame with the gap, display swaps, VBlanks and the most frequent places |
| `GOW_LOADER_DIAG=1` | Adds the level loader (0x185A20), its disc reads and the 989snd command count to the `[gow-hang]` report |
| `GOW_FAST_DISPATCH=0` | Turns off the inline call dispatch of the generated code (`dispatchGuestBranch` in `ps2_runtime.h`); every guest call then goes through the slow path. Also off with `GOW_GAP_DIAG` or `PS2X_GS_WORKER_TRACE`, and for the `GOW_PERF_FRAME_PC` function |
| `GOW_EE_WAIT=1` | The EE sleeps while waiting for the frame (changes game pacing) |
| `PS2X_IOP_NATIVE`, `PS2X_IOP_NATIVE_STATS` | Native IOP module use and its hit statistics |
| `PS2X_THREAD_PRIORITY=0\|1` | The GL thread runs above normal priority by default only when the process may use 4 or more logical processors (its affinity mask); `0`/`1` force it off/on |
| `GOW_GS_STAGE_SPIN_US=<us>` | How long the GS stage spins before sleeping when idle. Default 50 with 4 or more usable processors, 0 below (on 2 cores the spin took the CPU the EE and VU1 need) |
| `PS2X_GS_PIPELINE=0\|1` | The GS frontend on its own stage thread (default with 4 or more usable processors) or on the GS worker (default below) |
| `GOW_HOST_WAIT_FRACTION=yield\|sleep` | How the EE waits the last fraction of a millisecond before a pacing deadline: `yield` (default with 4 or more usable processors: precise, keeps the thread runnable) or `sleep` (default below: a high-resolution timer, leaves the CPU to the other threads) |

## Input and features

| Variable | Effect |
|---|---|
| `PC_KEYS="action=code,code;..."` | Keyboard/mouse map (`1000+n` = mouse buttons) |
| `PC_DMC5_CONTROLS`, `PC_PAD_FACE_MAP` | Pad preset and the A/B/X/Y swap |
| `GOW_PAD_DEVICE=<GLFW GUID>`, `GOW_PAD_LIST_FILE=<path>` | Controller picker: the one device the game reads (a GUID of the list below; absent or past raylib's four slots: automatic choice, one `[pad]` line), and the JSON file where the game lists the joysticks GLFW sees (index, name, GUID, SDL mapping yes/no, counts of buttons, hats and axes, the choice made), rewritten when the list changes. Both are set by the launcher (`gow-core/src/controller.rs`) |
| `GOW_PAD_SDL_MAPPING=<guid>,<name>,<fields>` | A hand-made SDL mapping for the chosen device when GLFW has none (`a:b0,start:b7,dpup:h0.1,leftx:a0,lefttrigger:+a2,...`, GLFW's raw numbering). The game adds `platform:` itself and hands the one line to `glfwUpdateGamepadMappings` once; anything that is not exactly one line of that shape is ignored with a `[pad]` line. While `pads/probe` next to the device list was touched in the last 10 s the game also writes `pads/live.json` (raw buttons held, hats and axes off rest, a `beat`) for the launcher's mapping dialog |
| `PC_PAD_MAP="source=target;..."` | Full gamepad map: sources `a b x y lb rb lt rt back start ls rs up down left right`, targets `cross circle square triangle l1 r1 l2 r2 select start l3 r3 up down left right none`. A source once, a source not listed presses nothing, `start` must be a target. Valid: replaces the two rows above. Invalid: ignored with one `[pad] PC_PAD_MAP ignored: <reason>` line. Rules in `gow_pad_map.h`, copy in `gow-core/src/padmap.rs` |
| `PC_CHEATS=god_mode,...`, `PC_CHEATS_SHORTCUT`, `PC_CHEATS_DIAG`, `PC_CHEATS_OPEN_IN`, `PC_CHEATS_OPEN_PAGE=saves` | Cheats at startup, menu shortcut, diagnostics, opening the menu at a time (and on its Saves page) for captures |
| `PC_LANGUAGE=en\|pt-BR`, `PC_MENU_FONT=<ttf>` | Cheat-menu language and font |
| `GOW_TARGETING=0`, `GOW_TARGETING_DIAG`, `GOW_TARGETING_TEST`, `GOW_TARGETING_STICK`, `GOW_TARGETING_DUMP` | Target lock |
| `GOW_CANCEL=0`, `GOW_CANCEL_EVASION`, `GOW_CANCEL_COMBO`, `GOW_CANCEL_BUFFER`, `GOW_CANCEL_DIAG`, `GOW_CANCEL_SEGMENT`, `GOW_CANCEL_AXIS`, `GOW_CANCEL_DUMP`, `GOW_CANCEL_WATCH` | Agile combat |
| `PC_RA_ENABLED`, `PC_RA_USERNAME`, `PC_RA_PASSWORD`, `PC_RA_HARDCORE`, `PC_RA_NOTIFICATIONS` | RetroAchievements |

## Content and paths

| Variable | Effect |
|---|---|
| `GOW_ISO=<file>` | Disc image to read from (otherwise the extracted files are used) |
| `GOW_MC0`, `GOW_MC1` | Memory-card images; `GOW_MC0=0` empties slot 1 |
| `GOW_SKIP_FMV=1` | Skips the game's movies (for load-time investigation) |
| `GOW_MOVIE_EOF=0`, `GOW_MOVIE_EOF_WAIT_MS=<ms>` | Turns off the end-of-movie handling (`gow_movie.cpp`: the stream read reports the end of the file, the completion test waits for the decoder's last pictures, at most 1500 ms) |
| `GOW_SAVE_BACKUP=0`, `GOW_SAVE_BACKUP_KEEP=<n>`, `GOW_SAVE_BACKUPS_DIR=<dir>`, `GOW_SAVE_DIAG=1` | Save history ([`MEMORY-CARD.md`](MEMORY-CARD.md#save-history)): off, how many automatic copies are kept (30; 0 keeps all), where they go (`<card folder>/save-backups`), and a `[gow-save]` line for every completed index write |
| `GOW_KEEP_AWAKE=0` | Lets Windows turn the display off or sleep while the game is on screen. By default the game asks it not to (`SetThreadExecutionState`, display and system), because a gamepad does not count as activity for Windows; the request is released while the window is minimized. `[gow-awake]` in the log. Windows only |
| `GOW_LOAD_SLOT=<0-3>`, `GOW_SAVE_LOAD=0` | Loading a slot from outside the game's menus ([`MEMORY-CARD.md`](MEMORY-CARD.md#loading-a-slot-from-outside-the-games-menus)): start the game on that slot, or turn the feature off (the launcher's Load and the F2 Saves page). `GOW_SAVE_DIAG=1` also logs each step (`[gow-load]`) |
| `GOW_FAST_BOOT=1` | Skips the entry animation |
| `GOW_TEXTURE_REPLACE`, `GOW_TEXTURE_REPLACE_SYNC`, `GOW_TEXTURE_REPLACE_{CPU,GPU}_MB`, `GOW_TEXTURE_REPLACE_UPLOAD_MB`, `GOW_TEXTURE_REPLACE_DIAG` | Custom texture packs |
| `GOW_PAD_TEST`, `GOW_PAD_TEST_NO_CAPTURE`, `GOW_PAD_SCRIPT`, `GOW_PAD_CAPTURES`, `GOW_PAD_LOG` | Automated pad input and GS frame captures |
| `GOW_PAD_DIAG=1` | `[gow-paddiag]` lines for "my pad does nothing" reports: every joystick GLFW lists (name, GUID, SDL mapping, resting axes), raw and mapped button/hat/axis changes, raylib's gamepad buttons, and what the game's pad read receives (source and buttons) |
| `GOW_HOST_SHOT`, `GOW_HOST_SHOT_ON_CHANGE` | Window screenshots at a frame number |

## Instrumentation

| Variable | What it prints |
|---|---|
| `GOW_PERF_DIAG=1`, `GOW_PERF_FRAME_PC=<pc>` | `[gow-perf]` per-second split: EE, VU1, GS, upload, flip rate |
| `GOW_DELTA_DIAG=1` | `[gow-speed]` game time vs wall time (see [`../PERFORMANCE.md`](../PERFORMANCE.md)) |
| `GOW_FRAMES_HITCH_LOG=1` | `[gow-frames] stutter of N ms at t=...` for every game frame over 50 ms (alone, without the 5 s report), with what that frame spent: the `[gow-perf]` buckets of every thread (`ee`, `vu`, `gs`, `iop`, waits, `upload`; they need `GOW_PERF_DIAG=1`), the GS texture decodes with their upload (`tex_decodes`, `tex_ms`, `tex_ktexels`) and the new render targets |
| `GOW_GAP_DIAG=<ms>` | `[gow-gap]` when two guest function entries are more than `<ms>` apart: the pc entered before the gap (where the time went) |
| `GOW_LOG_DIRECT=1` | Write stdout/stderr straight to the files again. By default (0.3.0) they go through a pipe and a writer thread, because a direct write could stall the game for 50-100 ms |
| `GOW_SAMPLING=1` | Periodic subsystem sampling (`ps2-dev perf-summary` parses it) |
| `GOW_VU1_STATS=1`, `GOW_VU1N_DIAG=1`, `GOW_VU1_BUDGET_DIAG=1` | VU1 native/interpreted coverage, native block hits |
| `GOW_GS_HW_STATS=1`, `GOW_GS_HW_PROFILE=1`, `GOW_GS_QUEUE_DIAG`, `GOW_GS_PRESENT_PHASE_DIAG` | GS backend counters and queue timing |
| `GOW_IO_HIST=<s>`, `GOW_IOP_IO_HIST=<s>` | Which hardware registers the game still touches, within `<s>` seconds |
| `GOW_VIF_DIAG=1`, `GOW_XGKICK_IMMEDIATE=1`, `GOW_VU1F_*` | VIF/GIF packet diagnostics |
| `GOW_AUDIO_STATS=1`, `GOW_IOP_DRIFT=1`, `GOW_SND_REGLOG=1`, `GOW_SND_DIAG=1` | Sound: frame counts, IOP drift, register writes |
| `GOW_SND_VOICE_DIAG=1` | Sound: `[gow-voices]` every 5 s with the SPU2 voices taken, in release and stuck in release |
| `GOW_MPEG_DIAG=1`, `GOW_MPEG_PACE`, `GOW_MOVIE_OVERRIDE` | Movie playback and A/V sync |
| `GOW_MENU_DIAG=1` | Read-only observers of the original UI (HUD object, message bank, Flash movies; `[gow-menu]`, at most 64 samples each) |
| `GOW_NATIVE_CHEATS_MENU=1` | Adds **Port Options** to the original pause menu (below Options) and to the title menu (after Treasures): a list with **Cheats** (a screen with the nine cheats of the F2 menu, each ON/OFF, shared with it) **In Game Overlay** (a screen with the FPS, GPU, CPU and RAM lines of the performance overlay, each ON/OFF, session only) and **Image Adjustments** (sliders for the Shade Boost: `PC_BRIGHTNESS`, `PC_CONTRAST`, `PC_GAMMA`, `PC_SATURATION` give the start values, the page changes them live, session only; the scaler shader is then always loaded); needs the retail `FLP_HUD`, off under RetroAchievements Hardcore. Opt-in. See `NATIVE_MENU_RESEARCH.md` |
| `GOW_INGAME_STATE=<file>` | Set by the launcher (`<data folder>/ingame-state.json`): the game writes the state of the cheats, the four overlay lines, the four image adjustments, the image quality (resolution, upscaling, sharpness, FXAA) and the frame rate (FPS limit, VSync, Early FINISH) there, 0.4 s after the last change and once more at exit, and the launcher reads and deletes it when the game ends (`gow-core/src/ingame.rs`). Unset: nothing is written. `GOW_PERSIST_DIAG=1` logs each write (`[gow-persist]`). The game must be able to write there (the low-integrity test builds can only write in their working folder) |
| `GOW_MENU_REPEAT=0` | Turns off the repeat of Left / Right held in the menus (outside gameplay a direction held 0.4 s is released and pressed again every 0.12 s, so the sliders of Image Adjustments and the game's own volume bars can be held; d-pad, keyboard and left stick) |
| `GOW_MENU_DUMP_MOVIE=<size>` | With `GOW_MENU_DIAG=1`: logs every movie the game builds (`BuildMovie`: resource, size, flags, table counts) and writes the buffer of the one with that size to `movie_dump.bin` in the working folder (research aid: compares the in-memory movie with the PAK) |
| `GOW_RENDER_DIAG=1`, `GOW_PATH_DIAG=1`, `GOW_ANM_DIAG=1`, `GOW_SKEL_DIAG=1`, `GOW_MODEL_DIAG=1` | Renderer, node search, animation, skeleton |
| `GOW_GS_REPLAY_TRACE`, `GOW_GS_REPLAY_AFTER`, `GOW_GS_REPLAY_SECONDS`, `GOW_GS_REPLAY_TEXFLUSH` | Record a GS command stretch. The recording works, but the consumer (`replay_gs`) does not build in this tree; use `GOW_VIF_CAPTURE` plus `replay_vif_chain` for VIF1/VU1 work |
| `PS2X_EE_DIFF*`, `PS2X_EE_WATCH*` | Differential EE runs against PCSX2's interpreter |
| `GOW_VU1_PCSX2=1` | Run VU1 on PCSX2's interpreter (A/B reference) |

There are many more single-purpose switches (GS dumps, VU1 recording, EE traces). The complete, authoritative list is
one command away:

```bash
grep -rhoE 'getenv\("([A-Z0-9_]+)"\)' RustPortPS2/native --include=*.cpp --include=*.h --include=*.inl \
  | sed -E 's/.*"([A-Z0-9_]+)".*/\1/' | sort -u
```

Read the code that uses a variable before relying on it: most dump files are game data and must stay local.
