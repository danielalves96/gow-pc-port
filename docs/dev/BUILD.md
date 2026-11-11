# Building and testing

Two ways to build: let the installers do it (they also generate the code) or drive CMake/cargo yourself. Both produce
the same `gow-2005`.

> The repository contains no game files. Building the **game** requires your own ISO; building and testing the
> **recompiler, the tools and the launcher** does not.

> Windows is the primary target; the Linux build works but is **experimental** (see [`../STATUS.md`](../STATUS.md)).

## Toolchains

| | Windows | Linux |
|---|---|---|
| Compiler | clang from [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) 20261020 UCRT (default since 0.3.0; `install.ps1` downloads it, SHA-256 pinned), or MSVC 2022 with `-Compiler msvc` (Build Tools; `vcvars64.bat` loaded) | GCC 13+ (`g++`, default in `install.sh`) or Clang 17+ (`./install.sh --compiler clang`, `CC=clang CXX=clang++`, or `-DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++`) |
| Build system | CMake 3.21+ with `cmcldeps.exe` next to it (any official build has it), Ninja | CMake 3.21+, Ninja |
| Rust | stable (for the recompiler and the tools) | stable |
| Launcher | Node 22+, Rust stable, Tauri prerequisites | Node 22+, Rust stable, `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev patchelf` |

## The fast path: the installers

```powershell
install.cmd -Iso "D:\God of War.iso"                 # Release, logs off
install.cmd -Iso "D:\God of War.iso" -FastDev        # RelWithDebInfo, logs on, console visible
```

```bash
./install.sh "/path/God of War.iso"
./install.sh --compiler clang --pgo on               # Clang build with the full PGO training + optimization pass (optional)
./install.sh --optimize 300                          # repeat the capture pass for 300 s
```

They run, in order: extract the ISO → apply the VU1 manifest → `ps2_recomp` → configure CMake → build → optionally
capture and rebuild. Each step is skipped if it is already done, so re-running is cheap.

## The shipped speed profile

Clang builds use PGO. `RustPortPS2/native/pgo/runtime.profdata` (2.3 MB, `*.profdata` is binary in `.gitattributes`) is the
profile every clang build uses by default (`PS2X_PGO=auto` in `CMakeLists.txt`: GCC and MSVC ignore it; the installers pass
`shipped`, `use`, `generate` or `off` explicitly). It was made from a full profile trained on the opening scene, keeping only
this repository's own runtime and dropping everything derived from the game, so it can live in the repository and the
installation never needs the game to open by itself. A full profile trained on the player's PC (`install.ps1 -Pgo on`, the
launcher's Optimize now: `build/pgo/gow.profdata`) wins over it when present.

Measured (final build, launcher environment, 60 FPS, opening scene): the full profile trained on the PC gives, on 4 cores,
VU1 -23 %, EE -20 %, GS -36 % per frame; the shipped runtime-only one gives GS -24 % (VU1 about -2 %, EE about 0), see
`TOASTER_PLAN.md` item 6.

**Refreshing it** (after runtime changes, before a release; a function that changed since the profile was made, or that it
never saw, is just compiled without profile data, and the `-Wno-profile-instr-*` flags keep the build quiet, so a stale
profile only loses part of its gain):

1. Build and train a full profile with the final configuration: `install.cmd -Pgo on -RetrainPgo` (it opens the game by
   itself for 4 minutes: do it on a PC you are not using).
2. `scripts/make-runtime-profile.ps1` (defaults: `RustPortPS2/native/build/pgo/gow.profdata` in, `RustPortPS2/native/pgo/runtime.profdata`
   out). It runs `llvm-profdata merge --no-function='sub_[0-9A-F]{8}|entry_[0-9A-Fa-f]+|vu1n_part|vu1c_gen|IL[jmiy][0-9]{3,}E'`
   (the recompiled EE functions, the generated VU1 blocks and compiled programs, and templates instantiated with microcode words),
   then audits the result: it lists the function names with `llvm-profdata show --all-functions` and fails if one is generated
   code, and fails if any function has indirect call sites in `show --ic-targets` (value-profile data could hold hashes of
   generated functions; to drop it, `merge --text`, zero the indirect-call sections and merge the text back with `-o`).
3. Commit the file and say in the message which commit the runtime sources were at and what the audit showed.

On Linux the same two commands work by hand: `llvm-profdata merge --no-function='<the regex above>' -o
RustPortPS2/native/pgo/runtime.profdata build/pgo/gow.profdata`, then `llvm-profdata show --all-functions` and
`show --ic-targets --all-functions` to check the names and the indirect call sites. Never commit the full profile.

## Manual build of the runtime

The installer's steps, done manually (from the repository root, after `game/` exists):

```bash
# 1. VU1 micro-memories from the ELF (no game session needed)
cargo build --release -p ps2-tools --manifest-path RustPortPS2/Cargo.toml
RustPortPS2/target/release/ps2-tools vu1-manifest apply game/SCUS_973.99 \
  RustPortPS2/games/scus-97399/vu1.manifest vu1_captures
cp RustPortPS2/games/scus-97399/vu1-heads.list "$(dirname vu1_captures)/vu1n_heads.txt"

# 2. Recompile the game
cargo build --release -p ps2-recomp-cli --manifest-path RustPortPS2/Cargo.toml
mkdir -p build-gen/out
cp game/SCUS_973.99 build-gen/ ; cp RustPortPS2/games/scus-97399/funcmap.csv build-gen/
sed -e "s|@ELF@|$PWD/build-gen/SCUS_973.99|" -e "s|@MAP@|$PWD/build-gen/funcmap.csv|" \
    -e "s|@OUT@|$PWD/build-gen/out/|" RustPortPS2/games/scus-97399/gow-port.toml > build-gen/recomp.toml
RustPortPS2/target/release/ps2_recomp build-gen/recomp.toml

# 3. Copy the generated code where CMake expects it
cp build-gen/out/*.cpp RustPortPS2/native/ps2xRuntime/src/runner/
cp build-gen/out/*.h   RustPortPS2/native/ps2xRuntime/include/

# 4. Configure and build
cd RustPortPS2/native
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DPS2X_ENABLE_RUNTIME_LOGS=OFF -DPS2X_BUILD_TEST=OFF \
  -DPS2X_RUNNER_UNITY_BUILD_BATCH_SIZE=8 \
  -DPS2X_VU1_MICROCODE_DIR="$PWD/../../vu1_captures" \
  -DPS2X_VU1_GENERATOR_SOURCE="$PWD/tools/vu1/generate_vu1.cpp" \
  -DPS2X_TOOLS_EXE="$PWD/../../target/release/ps2-tools"
ninja -C build ps2EntryRunner          # produces build/ps2xRuntime/gow-2005
```

For iteration, `ninja -C build ps2EntryRunner` alone is enough after a source change. **Never edit the generated
files** (`src/runner/*.cpp`, `src/native/iopn_*`, `build/*/vu1*_generated`): they are overwritten.

## CMake options that matter

| Option | Default | Use |
|---|---|---|
| `CMAKE_BUILD_TYPE` | - | `Release` for playing/measuring; `RelWithDebInfo` for debugging (`-FastDev` does this) |
| `PS2X_ENABLE_RUNTIME_LOGS` | `ON` | The installers pass `OFF`; `ON` adds the `[PS2Runtime]`-style logs and is much slower |
| `PS2X_ENABLE_AGRESSIVE_LOGS`, `PS2X_ENABLE_IOP_RPC_TRACE` | `ON` | Installers pass `OFF` |
| `PS2X_BUILD_TEST` | `ON` | Builds `ps2xTest`; the installers pass `OFF` |
| `PS2X_ENABLE_DEBUG_UI` | `ON` | Dear ImGui/rlImGui panel (F1) |
| `PS2X_ENABLE_RUNNER_UNITY_BUILD`, `PS2X_RUNNER_UNITY_BUILD_BATCH_SIZE` | `ON`, `32` | Unity build of the ~6,400 units. The installers use `8` for `-Destination`/Release parity and lower memory |
| `PS2X_ENABLE_RUNNER_PCH` | `ON` | Precompiled header; changing a CMake file regenerates it |
| `PS2X_ENABLE_SCCACHE` | `ON` | Uses `sccache` when it is on `PATH` |
| `PS2X_VU1_MICROCODE_DIR` | empty | Folder with the `*.bin` micro-memories. **Empty means VU1 runs interpreted** |
| `PS2X_VU1_GENERATOR_SOURCE` | empty | `tools/vu1/generate_vu1.cpp` |
| `PS2X_TOOLS_EXE` | empty | `ps2-tools` (native VU1 block generator) |
| `PS2X_VU1_MAX_MEMORY_MB` | `0` (unlimited) | RAM cap for the generator; installers pass 4096 or 8192 |
| `PS2X_ENABLE_FFMPEG` | `OFF` | Alternative MPEG-2 decoder; libmpeg2 is the default |
| `PS2X_ENABLE_LTCG` | `OFF` | MSVC `/GL /LTCG`: **can take hours to link**; keep it off |
| `PS2X_FAST_MATH_AVX2` | `OFF` | MSVC `/fp:fast /arch:AVX2`, not validated with the game; the released build does **not** require AVX2 |
| `PS2X_VU1_AVX2` | `OFF` | Same idea for the VU1 microcode |
| `PS2X_SHOW_WINDOWS_CONSOLE` | `ON` | Console window on Windows; the installers pass `OFF` |
| `PS2X_PGO`, `PS2X_PGO_PROFILE` | `auto` | Clang only: `auto`/`shipped` = the profile in `native/pgo/`, `use` = the one in `PS2X_PGO_PROFILE`, `generate` = instrumented training build, `off` = none (see the next section) |
| `PS2X_LTO_JOBS` | empty | GCC/Clang LTO link processes; the Linux installer limits it by RAM |

To check that VU1 took the fast path, look for this in the Ninja log: `[generate_vu1] ... images` with a number greater than
zero. Also make sure `vu1n_heads.txt` sits **one level above** `PS2X_VU1_MICROCODE_DIR`. Without the heads file, blocks
listed there are interpreted (~7 million native-block misses per 5 s in combat).

## Tests

```bash
# Recompiler, tools and developer tools (no game files)
cd RustPortPS2
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt -p ps2-config -p ps2-elf -p ps2-r5900 -p ps2-recomp -p ps2-recomp-cli -p ps2-parity -p ps2-tools -p ps2-devtools -- --check

# Gates
cargo run --release -p ps2-parity -- ...     # byte-identical tree (see RustPortPS2/docs/PARITY.md)
cargo run --release -p ps2-devtools -- validate-config
cargo run --release -p ps2-devtools -- check-gs-replay <gow-2005> <capture.bin>
cargo run --release -p ps2-devtools -- check-vu1-compiled <gow-2005>
```

`ps2-dev` with no arguments prints every command (validators, `perf-summary`, `sampling`, `gif-inspect`,
`vif-extract`, `cancel-line`, `gen-call-list`, test drivers).

C++ tests (these need the runtime built with `PS2X_BUILD_TEST=ON`):

```bash
cmake -S RustPortPS2/native -B RustPortPS2/native/build-test -DCMAKE_BUILD_TYPE=RelWithDebInfo -DPS2X_BUILD_TEST=ON
ninja -C RustPortPS2/native/build-test ps2x_tests
RustPortPS2/native/build-test/ps2xTest/ps2x_tests                 # the runtime test binary
ctest --test-dir RustPortPS2/native/build-test --output-on-failure # the ps2xIOP tests (-DPS2X_IOP_BUILD_TESTS=ON)
```

`ps2x_tests` covers the VU1 compiled path, the EE/kernel path, the pad2 packet, texture replacement and the audio
simulator; the GS replay fragments under `native/tests/*.cmake` are not part of the default build.
`ps2-dev validate-tests native/tests/known_failures.txt` reads the test binary's output on stdin and fails only on
tests that are not in the known-failure list.

Launcher:

```bash
cd gow-launcher
npm ci && npm test && npm run build
npm run check-kit             # every file the installers read is in the staged kit
cargo test --workspace
npx tauri dev                 # the real app
```

## CI

| Workflow | Runs | Does not run |
|---|---|---|
| `.github/workflows/rust-port.yml` | `cargo fmt/clippy/test` on Windows and Linux for every crate | the C++ runtime, the installers, the game |
| `.github/workflows/gow-launcher.yml` | Type-check, Vitest, kit and Windows-installer checks, `cargo test` for the launcher; tags `launcher-v*` build NSIS/deb/AppImage drafts | the C++ runtime, the game |

`pwsh -NoProfile -File scripts/ci/analyze_ps1.ps1` parses every `.ps1` with the PowerShell parser; run it after editing
an installer. `scripts/ci/test-install-ps1.ps1` tests the pure decisions of `install.ps1` (the job count, the processor
check, the Visual Studio loader with a fake `cmd` first in `PATH`), and `bash scripts/ci/test-install-sh.sh` those of
`install.sh`. `.github/workflows/install-ps1.yml` runs the first two on Windows (PowerShell 5.1 and 7) and
`install-sh.yml` the last one on Linux, whenever an installer or a script changes.

**Nothing in CI compiles the runtime, runs an installer or plays the game.** The byte-identical parity gate runs in CI
on a synthetic fixture; against the retail ELF it only runs locally, because the game cannot be uploaded.

## Common problems

| Problem | Cause / fix |
|---|---|
| `ps2_recomp` "does not open paths with non-ASCII characters" | Work from an ASCII path; the Windows installer falls back to `%SystemDrive%\PCGen` |
| CMake "does not match the source" | The build folder was configured from another checkout: delete `build/CMakeCache.txt` (the installers do it automatically) |
| The link takes hours (Windows) | `PS2X_ENABLE_LTCG=ON`, or the machine is swapping. Keep LTCG off and lower `-Jobs` |
| Out of memory during LTO (Linux) | Lower `PS2X_LTO_JOBS` and `--jobs` |
| VU1 is slow / "interpreted" in the log | `PS2X_VU1_MICROCODE_DIR` is not set, it contains no `*.bin`, or `vu1n_heads.txt` is missing |
| Changes to a header rebuild everything | Expected: ~6,400 units include it. Prefer `.cpp` changes; batch header/CMake edits |
