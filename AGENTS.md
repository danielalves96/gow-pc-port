# Guide for agents and contributors

A port of *God of War* (PS2, `SCUS-97399`) built by static recompilation. The recompiler (`RustPortPS2/`) reproduces
[PS2Recomp](https://github.com/ran-j/PS2Recomp)'s C++ output byte for byte; the C++ runtime lives in
`RustPortPS2/native/` and is built on the runtime of
[Waffle565/God-Of-War-Native-Port](https://github.com/Waffle565/God-Of-War-Native-Port). This repository contains no
game files and no generated C++. Release status and known limits: [`docs/STATUS.md`](docs/STATUS.md).

## Where each thing goes

| Change | Where |
|---|---|
| Replacements and diagnostics of game functions | `RustPortPS2/native/ps2xRuntime/src/game/` (`gow_overrides.cpp` binds each address; one `gow_*.cpp` per area; part of the `ps2EntryRunner` target, not copied by the installer) |
| PC features added on top of the runtime (launcher, widescreen, cheats, target lock, combat) | `RustPortPS2/native/ps2xRuntime/src/lib/gow_*.cpp`, wired in `gow_integration.cpp` |
| Runtime and IOP (C++, built with CMake) | `RustPortPS2/native/` (`ps2xRuntime`, `ps2xIOP`, `ps2xTest`) |
| Recompiler (`ps2_recomp`) | `RustPortPS2/crates/ps2-recomp*` (+ `ps2-config`, `ps2-elf`, `ps2-r5900`), see `docs/dev/ARCHITECTURE.md` |
| Installer helpers (ISO extraction, ELF check, VU1 and IOP generators) | `RustPortPS2/crates/ps2-tools` (binary `ps2-tools`; the installers need no Python) |
| Byte-identical gate of the recompiler | `RustPortPS2/crates/ps2-parity`, see `RustPortPS2/docs/PARITY.md` |
| Developer tools (config/test validators, profile and capture summaries, call-list generator) | `RustPortPS2/crates/ps2-devtools` (binary `ps2-dev`; run `ps2-dev` for the list) |
| C++ tests and test benches of the runtime (GS replay, VU1, pad2), the VU1 generator, the sampler | `RustPortPS2/native/tests/`, `RustPortPS2/native/tools/` |
| PowerShell helpers (builds) and the CI script checker | `scripts/` |
| Game data: recompiler functions, stubs and patches (`gow-port.toml`), function map (`funcmap.csv`), VU1 pieces of the ELF (`vu1.manifest`) and dynamic VU1 entry points (`vu1-heads.list`) | `RustPortPS2/games/scus-97399/` |
| Custom textures (PCSX2-format replacement packs: hashing, pack, GL hook) | `RustPortPS2/native/ps2xRuntime/src/lib/gs/gpu/gs_texture_replace.cpp`, hooked in `gs_hw_backend.cpp` (`replacementFor`); see `docs/TEXTURES.md` |
| Launcher and installation wizard | `gow-launcher/` (Rust `gow-core` + Tauri + React), see `gow-launcher/README.md` and `docs/LAUNCHER.md` |
| Installation | `install.sh` (Linux), `install.ps1` + `install.cmd` (Windows) |
| Documentation | the two READMEs (`README.md`, `README.pt-BR.md`; they must stay in sync), `docs/` (players) and `docs/dev/` (contributors) |

## Rules

- Mark changes to code that comes from those upstream projects with a `// GOW-Port:` comment.
- Rust runs outside the game process (recompiler, installer tools, launcher, developer tools); C++ runs inside it (runtime, IOP,
  overrides). The overrides call the generated functions and register themselves at startup, so they stay C++ and in the
  `ps2EntryRunner` target. They declare the generated functions they wrap in `gow_guest_functions.h` instead of including
  `ps2_recompiled_functions.h`, so that the folder resolves its includes before the installer has generated anything.
- Do not hand-edit generated files (`RustPortPS2/native/ps2xRuntime/src/runner/*.cpp`, `ps2xIOP/src/native/iopn_*`,
  `build/*/vu1_generated`, `vu1n_generated`): they are regenerated.
- Changing a header included by the generated code (`ps2_runtime.h`, `ps2_memory.h`, `ps2_perf.h`...) recompiles the
  ~6,400 units: avoid it when a change to a `.cpp` is enough. The same happens with any `CMakeLists.txt` edit (the
  precompiled header is regenerated), so batch CMake changes.
- Overrides call the original function for everything they do not replace, and they must handle resumption after a
  checkpoint: when `ctx->pc` differs from the entry, call the original immediately.
- Optional diagnostics use an environment variable (`GOW_*_DIAG`) and a `[gow-xxx]` tag in the log.
- Everything in the repository is in English: documentation, comments, identifiers, file names, log messages. The only
  translations are the Brazilian Portuguese ones (`README.pt-BR.md`, the launcher and the
  in-game cheat menu strings). Commits are in English.

## Building and testing

`./install.sh` (or `install.cmd`) does everything. To iterate, run `ninja -C build ps2EntryRunner` from
`RustPortPS2/native/` (the CMake target keeps that name; it produces `gow-2005[.exe]`, with the Omega icon from
`ps2xRuntime/resources/`).
Rust: from `RustPortPS2/`, `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`; format with
`cargo fmt -p <crate> ...` (never `--all`: it would rewrite the vendored `rabbitizer`; the crate list is in
`.github/workflows/rust-port.yml`).
Tests with the game: `./run_gow.sh` with `GOW_PAD_TEST=1 GOW_PAD_SCRIPT="5:start,12:x,20:x"` (button presses),
`GOW_PAD_CAPTURES=...` (GS captures) and `GOW_HOST_SHOT=<frame>` (window capture).
Tests without the game: `cargo test --workspace` from `RustPortPS2/` (it also checks `RustPortPS2/games/` and the
call lists against the committed files; `ps2-dev validate-config` runs the same config check). The launcher:
`cargo test --workspace` and `npm test` from `gow-launcher/`. The Rust port: `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo fmt -p ps2-config -p ps2-elf -p ps2-r5900 -p ps2-recomp -p ps2-recomp-cli -p ps2-parity -p ps2-tools -p ps2-devtools -- --check`
from `RustPortPS2/` (never `cargo fmt --all`: it rewrites the vendored Rabbitizer). Its byte-identical gate runs on a
synthetic fixture in CI and on the retail ELF locally (`RustPortPS2/docs/PARITY.md`).

## Frame-rate cap: 120 FPS at most (non-negotiable)

- The goal is a steady 60 FPS without stutter on any PC, the weakest included. Nothing in the project runs the game or its
  window above **120 FPS**: the runtime (`kMaxFps` in `ps2xRuntime/src/lib/gow_frame.h`: `PC_LOGIC_FPS` and
  `PC_WINDOW_FPS` are capped there, and `PC_WINDOW_FPS=0` no longer means unlimited), the launcher (`MAX_FPS` in
  `gow-launcher/crates/gow-core/src/launch.rs`: limits 60 and 120; the window at 60 with the 60 limit, at the display's rate up to 120 with the 120 one), the
  in-game Frame Rate page (60 / 120) and every test or measurement.
- No "uncapped" tests either: `scripts/ab-perf.ps1` refuses `PC_LOGIC_FPS` above 120 and `PC_WINDOW_FPS` of 0 or above 120.
  Uncapped runs kept the GPU at 100 % until the AMD OpenGL driver crashed (`atio6axx.dll`) and measure nothing a player
  sees. Compare builds at 60 FPS with the launcher's real environment (`-Cores 4` for a weak PC), by the per-thread time
  per frame (`[gow-perf]`), the frames over 16.7 ms and the hitches (`GOW_FRAMES_HITCH_LOG=1`).

## Rules for living with the user's game (non-negotiable)

- **Never** close processes by name (`Get-Process gow-2005 | Stop-Process`): doing so has already killed the user's
  running game. Close only the PID that the script itself started.
- **Warn the user and wait for their go-ahead** before launching the game to test; if a `gow-2005` is already running, do
  not launch another. Each test competes for CPU/GPU and can cause audio dropouts and slowdowns in the game the user is
  playing.
- Run long builds at **Idle** priority (`ninja` and its children) and never touch the executable the user has open. Do
  not measure performance while anything else (a build, another session) is using the CPU.
- Detail and context: [`docs/dev/PERFORMANCE.md`](docs/dev/PERFORMANCE.md) and [`docs/STATUS.md`](docs/STATUS.md).

## Final game build (mandatory for measuring performance and for delivering)

The game build is the one `install.ps1` produces without `-FastDev`. Any other build is useless for judging smoothness
or audio:

- `CMAKE_BUILD_TYPE=Release`, `PS2X_ENABLE_RUNTIME_LOGS=OFF`, `PS2X_SHOW_WINDOWS_CONSOLE=OFF`,
  `PS2X_RUNNER_UNITY_BUILD_BATCH_SIZE=8`. (`-FastDev` uses `RelWithDebInfo` with logs ON and is only for iterating.)
- **Pre-generated (AOT) VU1, never interpreted**: configure with `PS2X_VU1_MICROCODE_DIR=<vu1_captures>` (the folder of
  `*.bin` files recorded while playing), `PS2X_VU1_GENERATOR_SOURCE=RustPortPS2/native/tools/vu1/generate_vu1.cpp` and
  `PS2X_TOOLS_EXE=<ps2-tools>` (the native block generator, `ps2-tools vu1n-gen`), with `vu1n_heads.txt` next to that
  folder (one level up). Without these, VU1 is interpreted and the game is much slower; without `vu1n_heads.txt`, combat
  produces ~7 M native-block misses every 5 s. To verify, the ninja log must show `[generate_vu1] ... images` with a
  count above 0, and `GOW_PERF_DIAG=1` must report a low `vu_ms`.
- **Always measure with the launcher's real environment**, not an invented one: `gow-launcher/crates/gow-core/src/launch.rs`
  (`BASE_ENVIRONMENT`: `PS2X_GS_WORKER=1`, `GOW_GS_FINISH_ASYNC=1`, `GOW_AUDIO_DEVICE=default`, `GOW_PERF_DIAG=1`,
  `GOW_PERF_FRAME_PC`, `GOW_NATIVE_CHEATS_MENU=1`) plus the user's settings (`settings.json` in `%APPDATA%\GOW_PC\Launcher`: internal
  scale, FXAA, filter, resolution). `cargo run -p gow-cli -- env` from `gow-launcher/` prints it. Without
  `PS2X_GS_WORKER=1` the GS runs inline with the EE, which causes audio dropouts and slowdowns that players never see.
  Confirm in the log that `[gs-worker] VIF1/GIF processing on a worker thread` appears, and check the `exepath` of the
  measured executable.
- Do not call a change a regression or an improvement without comparing it to a build **of the previous commit made with
  this same configuration** (same scene, `GOW_PERF_DIAG=1`).
- At the end of a session that touched performance, leave `RustPortPS2/native/build` configured as above (logs OFF).

## Legal

Never upload game files (ELF, IRX, ISO, `.PAK`), code generated from it, RAM/VRAM captures or the captured VU1 microcode.
`game/`, `vu1_captures/`, `build-gen/` and `logs/` are in `.gitignore`.
