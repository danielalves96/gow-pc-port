# Installing

There is no prebuilt binary of the game. Its code is **recompiled on your computer** from your own disc: this is a
static recompilation, not an emulator, so the disc is compiled into a native program instead of running in a virtual
PS2. On Windows a launcher does all of it for you, and two installer scripts cover people who prefer a terminal.

> [!IMPORTANT]
> This repository contains **no game files** (ISO, executable, IRX modules, `.PAK` data) and no code generated from
> them. You need your own legal copy of *God of War* for the PlayStation 2: the **USA** version, `SCUS-97399`.

> [!WARNING]
> **Linux support is experimental.** The Linux code path and installer exist in the repository and the game builds and
> runs there, but it is not validated to the same level as Windows: no CI job builds or tests the runtime on any
> platform, recent Linux builds have not been measured, vibration is not implemented, and the launcher's setup wizard,
> which drives `install.sh` on Linux, has never run on a real Linux machine. Treat Linux as prepared, not supported; see
> [`STATUS.md`](STATUS.md).

## What you need

| | |
|---|---|
| **Game** | Your own *God of War* (USA, `SCUS-97399`) disc image (`.iso`, about 8 GB, or a `.chd` of it). Rip it from your own disc with any DVD tool. Please don't pirate it |
| **Disk** | About **20 GB** free while installing (the ISO and the extracted disc are about 8 GB each, the build and the compiler about 2 GB); about 11 GB stay once the ISO is deleted |
| **CPU** | 64-bit x86 with **SSE4.1** (Intel Core 2 "Penryn", 2008, or newer; AMD "Bulldozer", 2011, or newer; the AMD Phenom II does not have it), 4 cores or more recommended. AVX2 is **not** required by the shipped build |
| **RAM** | 8 GB (16 GB recommended while building) |
| **GPU** | OpenGL 4.3 or newer (the renderer asks for 4.6 first and falls back down to 4.3; 4.6 is the tested path): any recent NVIDIA/AMD card, or the integrated graphics of a recent Intel/AMD CPU |
| **OS** | Windows 10/11 x64, or Linux: Arch/CachyOS, Ubuntu 24.04+, Fedora 40+ or similar (**experimental**, see above) |
| **Time** | Windows: the first build takes **about 10–20 minutes** (10 on a 6-core Core i5-12400F, longer on 4 cores), plus a few minutes for the disc and a 170 MB compiler download. Linux (g++, not measured on 0.3.0): 30–90 minutes. Later updates only rebuild what changed |

### Modified discs, patches and other editions

- **Modified discs** (a translation, an undub, a texture or other data mod) work when they kept the game's code: the
  wizard checks the code section of the executable (`.text`) against the USA disc, says "modified disc recognized", and
  installs it like the original.
  RetroAchievements stay off with any disc other than the original USA image, because they only recognise that one.
- **Patches shipped as an .xdelta file** (translations, undubs, texture and other data mods) are applied by the wizard
  itself. With the original ISO chosen, tick **Apply a patch (.xdelta)**, choose the patch and press **Apply patch**: the
  patched ISO is created in the same folder (about 8 GB more), checked like any ISO, and used for the installation. The original ISO is only read,
  and **Use the original** goes back to it. A patch made for another disc stops at the first block, a patch that changes
  the game's executable (widescreen hacks, cheats, code fixes) is refused, because the port runs the USA disc's own code,
  and either way nothing is left behind. The option is not shown for an ISO that is already modified (nor for a CHD until it is converted, and then only if its disc is the unmodified original). Patches made with secondary compression (`xdelta3 -S djw|fgk|lzma`) are not supported: ask for one made
  with `xdelta3 -S none`. Without the launcher, the same decoder runs from the command line:

  ```
  bin\ps2-tools.exe xdelta-apply patch.xdelta "God of War (USA).iso" "God of War (patched).iso"
  ```
- **CHD images** (`.chd`, the compressed format of `chdman` that many emulator collections use) are accepted as they
  are: drop, browse or let the wizard find the `.chd`. It is checked in place, without converting it (a second or so),
  and the wizard says whether the disc inside is the original, a modified disc or another game. Press **Convert**: the
  ISO is written **in the CHD's folder** under the CHD's name (about 8 GB more, about a minute on a 6-core CPU, with a
  progress bar and **Stop**), every block of the CHD and its SHA-1 are checked as `chdman verify` does, and the
  installation then uses that ISO; an ISO converted earlier is used again instead of converting twice. The CHD is only
  read. Both `chdman createcd` CHDs (`cdlz`, `cdzl`, `cdfl`, `cdzs`) and `createdvd` / `createhd` ones (`lzma`, `zlib`,
  `huff`, `flac`, `zstd`) work; CHDs older than version 5 and delta CHDs that need a parent CHD do not (`chdman copy`
  turns them into a complete version 5 one). From the command line, `install.cmd -Iso "D:\God of War.chd"` (or
  `./install.sh "God of War.chd"`) converts it to a temporary ISO in the install folder and deletes it once the disc is
  extracted; the converter alone is:

  ```
  bin\ps2-tools.exe chd-extract "God of War.chd" "God of War.iso"
  ```
- **Other regions and demos** (Japan `SLPM-67010`, Europe `SCES-53133`/`SCES-51533`, Asia `SCAJ-30010`, the USA and
  Europe demos) are recognised and named, but not supported yet: their code is different from the USA disc's.
- **Mods that change the code** (an executable with a different `.text`) are refused, because the port is built for the
  USA code.

The installation folder must not contain accents or special characters (the launcher enforces this; the Windows
installer works around it by moving the generated code to `%SystemDrive%\PCGen`).

## Windows: the launcher (recommended)

1. Download `God Of War - PC Launcher_<version>_x64-setup.exe` from the Releases page and run it. It installs for your
   user only (no administrator permission needed) and bundles WebView2, so it works on a clean Windows 10/11.
   The installer is not code-signed yet, so Windows SmartScreen shows "Windows protected your PC" and an "unknown
   publisher": click **More info**, then **Run anyway**. Code signing needs a paid certificate (or Azure Trusted
   Signing), which the project doesn't have yet.
2. Open the launcher. The **setup wizard** starts by itself:
   1. **ISO**: it looks for your ISO and verifies it. If it can't find the file, drop it on the window or use
      **Browse**.
   2. **Location**: where the game goes (`C:\Users\<you>\GodOfWar-PC` by default), ASCII path, 20 GB free.
   3. **Components**: since 0.3.0 there is nothing to install by hand. The game is built with a portable clang
      ([llvm-mingw](https://github.com/mstorsjo/llvm-mingw), about 170 MB, SHA-256 checked) that the installer downloads into
      the install folder, without administrator permission and without Visual Studio. CMake, Ninja and Git are handled the
      same way when the PC has no usable ones. The step only appears for an old kit without the prebuilt tools (`bin\`),
      which still needs Visual Studio 2022 Build Tools.
   4. **Install**: a progress bar and a cancel button. Finished steps are kept, so you can cancel and resume later.
3. Press **PLAY**. Check **Files** once: the executable, the runner and the memory card should all be detected.

The launcher's own documentation (tabs, options, paths, logs) is in [`LAUNCHER.md`](LAUNCHER.md).

**Updating the launcher.** When a newer launcher is published, a notice with an **Update** button appears in the title bar;
the button downloads and installs it and restarts the launcher (it waits while the game, an installation or an
optimization is running). **Other, Automatic updates** does the same by itself at start-up; it is off by default. Your
settings and the game are kept. Details: [`LAUNCHER.md`](LAUNCHER.md#updating-the-launcher).

**Game fixes after a launcher update.** The game itself is not replaced by a launcher update: it is a build made on your
PC. When the new launcher carries game fixes your build lacks, **Performance** shows a **Rebuild now** note. The rebuild
reuses what is already built, so it usually takes a few minutes; a fix that changes a header the whole game includes
rebuilds everything, which takes about as long as the first build (10–20 minutes on Windows). Until it finishes you keep playing the current build. Details:
[`LAUNCHER.md`](LAUNCHER.md#performance).

## Windows: the portable zip

For anyone who prefers not to run an installer, for example because SmartScreen warns about an unsigned one. It is the
same launcher and the same wizard; only the installation of the launcher itself is skipped.

1. Download `God Of War - PC Launcher_<version>_x64-portable.zip` from the Releases page. **Unblock it before
   unzipping**: right-click the zip, Properties, tick **Unblock**, OK. A zip downloaded by a browser carries the "from the
   internet" mark and Explorer hands it to every file inside, so without this step the unsigned `.exe` triggers the same
   SmartScreen warning as the installer (More info, Run anyway also works).
2. Unzip the whole folder to a path without accents or special characters (for example `C:\GodOfWarPC`) and open
   `God Of War - PC Launcher.exe`. Keep the `kit` folder next to it: the launcher copies the installation scripts from
   there on its first run. The wizard then works as described above.
3. What is different from the installer:
   - **No Start-menu entry and no uninstaller.** To remove it, delete the folder, `%APPDATA%\GOW_PC\Launcher` and the
     game folder you chose in the wizard.
   - **WebView2 is not included.** Windows 11 and up-to-date Windows 10 already have the Microsoft Edge WebView2
     Runtime; if the launcher does not open, install its Evergreen Standalone Installer from Microsoft.
   - **Settings stay in `%APPDATA%\GOW_PC\Launcher`**, not in the folder, so a new version can be unzipped over the old one
     (or next to it): settings carry over and the kit refreshes itself.
   - **It does not update itself.** It still shows the "new version available" notice, but the button opens the release
     page: download the new zip there and unzip it over the old folder.

Every release has `SHA256SUMS-windows.txt` and `SHA256SUMS-linux.txt` with the SHA-256 of the installers and the zip, built
by the same GitHub Actions run. Compare with `Get-FileHash <file>` (PowerShell) or `sha256sum -c` (Linux). The zip also
contains a `README.txt` with these steps, in English and Portuguese.

## Windows: the installer script

From a checkout of the repository, PowerShell 5.1 or 7:

```powershell
install.cmd -Iso "D:\God of War.iso"                 # extracts, recompiles and builds
install.cmd -Iso "D:\God of War.iso" -InstallTools   # first time: installs the C++ build tools too
install.cmd -Destination "D:\GodOfWar"               # keep the game data and the build out of the repository
```

| Option | Meaning |
|---|---|
| `-Iso <file>` | The disc image (`.iso`, or a `.chd`: converted to a temporary ISO that is deleted after the extraction). Without it, the first `.iso` (then `.chd`) in `game\` and then in the repository root is used |
| `-Destination <folder>` | Where `game\`, `build\`, `build-gen\` and `vu1_captures\` go. The generated C++ still lands in the repository |
| `-Compiler clang\|msvc` | `clang` (default since 0.3.0): the portable llvm-mingw, downloaded into the install folder; no Visual Studio is needed when prebuilt `ps2_recomp` / `ps2-tools` are available (the launcher's kit, or `-RecompExe` / `-ToolsExe`), otherwise Visual Studio's linker builds them with cargo. `msvc`: the Visual Studio compiler, as before 0.3.0 |
| `-Pgo auto\|on\|off` | Speed profile (clang only). `auto` (default): build with the profile this PC already trained, if any, else with the runtime-only profile that ships in the repository (`RustPortPS2/native/pgo/runtime.profdata`, made from our own runtime, nothing from the game: the installation never opens the game by itself). `on`: also train the full profile when there is none (the game opens by itself for the optimization run, then a second build uses it; about 20 % less VU1 and EE time per frame on top of the shipped profile). `off`: no profile. The trained profile is made on your PC from your game and is never distributed |
| `-RetrainPgo` | Train the full speed profile again even when one exists |
| `-InstallTools` | Downloads Microsoft's Build Tools installer (signature checked) and installs the C++ workload, when Visual Studio is missing and needed (`-Compiler msvc`, or the Rust tools must be built). A missing or unusable CMake or Ninja is not a reason: portable ones are downloaded first |
| `-NoOptimize` / `-Optimize` | Skip / force the optimization pass (see below) |
| `-Seconds <n>` | Duration of the optimization run (default 240 s) |
| `-Jobs <n>` | Parallel compiler processes. Default: the logical processors minus a reserve (2 from 8 processors, 1 from 4 to 7, none below), at most one per 2 GB of RAM. An explicit value skips the reserve |
| `-NormalPriority` | Keep the build at normal priority. By default the installer lowers its own priority to below normal, so the PC stays usable while the game builds (the compilers inherit it; the optimization run of the game runs at normal priority) |
| `-SkipCpuCheck` | Do not stop on a processor without SSE4.1. By default the installer checks it first, before downloading or building anything, and stops with an `[error]` line (exit code 1) explaining that the game cannot run there. Use the switch only if a virtual machine hides a feature the CPU really has |
| `-Vu1MemoryLimitGB <4-8>` | RAM cap for the VU1 generator (default 4 GB, `0` = unlimited) |
| `-Vu1BuildDir <folder>` | Use an existing VU1 microcode folder instead of `vu1_captures\` |
| `-FastDev` | `RelWithDebInfo`, logs on, console visible. For iterating; **not** for measuring performance |
| `-AllowModifiedElf` | Accept a modified `SCUS_973.99` (experimental builds) |
| `-RecompExe` / `-ToolsExe` | Use prebuilt `ps2_recomp` / `ps2-tools` instead of building them with cargo |

`install.cmd` is a thin wrapper around `install.ps1`; call the `.ps1` directly if your execution policy allows it.

## Linux (experimental)

1. Install the build dependencies. The easy way is to let the script do it: `./install.sh --install-deps` detects the
   distribution (Arch and relatives, Debian/Ubuntu and relatives, Fedora and relatives), lists what is missing and
   installs it through `sudo`. `./install.sh --check-deps` only reports. By hand:

   ```bash
   # Arch / CachyOS / Manjaro
   sudo pacman -S --needed base-devel cmake ninja git

   # Ubuntu / Debian
   sudo apt install build-essential cmake ninja-build git libgl-dev libx11-dev libxrandr-dev \
     libxinerama-dev libxcursor-dev libxi-dev libxkbcommon-dev libwayland-dev libasound2-dev

   # Fedora
   sudo dnf install gcc-c++ cmake ninja-build git mesa-libGL-devel libX11-devel libXrandr-devel \
     libXinerama-devel libXcursor-devel libXi-devel libxkbcommon-devel wayland-devel alsa-lib-devel
   ```

   The script looks the programs up in `PATH` and the libraries with `pkg-config` (GL, X11, Xrandr, Xinerama, Xcursor,
   Xi, xkbcommon, Wayland, ALSA). By default it uses `g++`; to compile with `clang++` instead, install `clang` (`sudo pacman -S clang`, `sudo apt install clang`, or `sudo dnf install clang`) and pass `--compiler clang` (or set `CC=clang CXX=clang++` / `GOW_COMPILER=clang`). Rust is only needed if you do not have prebuilt tools in `bin/` (see below). Python
   is **not** needed.

   Before building, it also checks the free space of the destination (about 20 GB; only a warning once the disc is
   already extracted) and the RAM (4 GB at least, a warning under 8 GB). `GOW_SKIP_RESOURCE_CHECK=1` skips both.

2. Put your ISO in `game/God of War.iso` or pass its path.
3. Run:

   ```bash
   ./install.sh                          # or: ./install.sh "/path/to/God of War.iso"
   ```

| Option | Meaning |
|---|---|
| `<iso>` (positional) | The disc image (`.iso` or `.chd`, as on Windows); otherwise the first `.iso` (then `.chd`) in `game/` then in the repository root |
| `--destination DIR` | Where `game/`, `build/`, `build-gen/` and `vu1_captures/` go (default: next to the script) |
| `--install-deps` | Install the missing system packages through `sudo` (otherwise it asks in a terminal, or prints the command) |
| `--check-deps` | Only report what is missing (`[missing]`, `[packages]` and `[install-command]` lines, exit code 2 if anything is) |
| `--no-optimize` | Skip the optimization pass |
| `--optimize [seconds]` | Force it, optionally for a different duration (default 240 s) |
| `--jobs N` | Parallel compiler processes. Default: the cores minus a reserve (2 from 8 cores, 1 from 4 to 7, none below), at most one per 2 GB of RAM. An explicit value skips the reserve |
| `--compiler g++\|clang` | C++ compiler (`g++` by default; `clang` uses `clang`/`clang++`, also selectable via `CXX=clang++` or `GOW_COMPILER=clang`) |
| `--pgo auto\|on\|off` | Speed profile (`clang` only, requires `llvm-profdata` from the `llvm` package when training; `g++` ignores it). `auto` (default): use `build/pgo/gow.profdata` if already present, else the runtime-only profile shipped in `RustPortPS2/native/pgo/runtime.profdata` (nothing from the game, no training run). `on`: also train the full profile during the optimization run and rebuild with it. `off`: disable PGO |
| `--retrain-pgo` | Train the full speed profile again even when `build/pgo/gow.profdata` already exists |
| `--vu1-memory-limit-gb N` | RAM cap for the VU1 generator in GB (default 4, `0` = unlimited) |
| `--normal-priority` | Do not lower the CPU and I/O priority of the long steps (by default they run under `nice -n 10`, and `ionice -c 3` when it works) |
| `--recomp-exe FILE` | Use a prebuilt `ps2_recomp` |
| `--tools-exe FILE` | Use a prebuilt `ps2-tools` |

The Rust tools come from `--recomp-exe`/`--tools-exe`, from `bin/` next to the script (the launcher's kit) or from
`cargo build`, in that order.

4. Play with `./run_gow.sh`, or install the `.deb`/AppImage and point the launcher at the installation.

The launcher's setup wizard also runs on Linux: it starts `install.sh` with the ISO and the destination you chose and
shows the same eight stages. When packages are missing it lists them with the command for your distribution (it can't
run `sudo` for you), and you press **Check again** after installing them. This path has unit tests (the arguments, the
report of `--check-deps` and the wizard step) but has not been run on a real Linux machine, so if it fails, the script
above is the fallback. The launcher can also find and start an installation made by the script (**Files → Find files
automatically**).

## What the installation creates

| Folder / file | Contents |
|---|---|
| `game/` | The extracted disc: `SCUS_973.99`, the data files, `.disc_layout` (sector map), `IOP_MOD/` and, after your first save, `Mcd001.ps2` |
| `build-gen/` | Work folder of the recompiler: the filled-in `recomp.toml`, `recomp.log`, the generated C++ in `out/`, `optimize.log` and the captured IOP modules |
| `build/` | The CMake/Ninja tree and the game: `build/ps2xRuntime/gow-2005` (`gow-2005.exe` on Windows) |
| `vu1_captures/` | VU1 micro-memory images (16 KB each) that let the vector unit run native code |
| `build-rust/` | `ps2_recomp` and `ps2-tools` when they were built from source |
| `textures/replacements/` | Where a custom texture pack goes ([`TEXTURES.md`](TEXTURES.md)) |
| `toolchain/` | Portable Git, CMake and Ninja (Windows only, each only when the PC lacked a usable one) |
| `RustPortPS2/native/ps2xRuntime/src/runner/` | The generated C++ of the game. **Always in the repository**, even with `-Destination` |

On Windows, `build\ps2xRuntime\game-elf.sha256` records which ELF the build belongs to; the launcher checks it before
starting the game.

## The optimization pass

The game's vector unit (VU1) and its I/O processor (IOP) can run either interpreted or as native code. The installer
already does the important half automatically:

- **VU1, always**: the microcode is extracted from the ELF itself using the committed manifest
  (`RustPortPS2/games/scus-97399/vu1.manifest`), before the first build. No game session is needed.
- **IOP and extra VU1, optionally**: `-Optimize` / `--optimize` starts the game with automatic button presses for 240
  seconds, records the code it executes, and rebuilds with it. Run it again after playing new areas to convert more of
  the game to native code; the port gets faster each time.

The launcher does the same thing in the background, after you close the game (**Performance → Background
optimization**), and shows a percentage. *Also while playing* is off by default because a rebuild with the game open
freezes the picture.

> While the optimization run is going, the game opens by itself and plays the beginning of the first level. **Do not
> touch the keyboard or the controller** until the window closes.

## Updating

```bash
git pull
./install.sh        # Windows: install.cmd
```

Only changed files are rebuilt. The extracted game, the recorded VU1/IOP code and your memory card are kept.

**From 0.2.9 or older to 0.3.0 (Windows).** 0.3.0 compiles the game with clang instead of Visual Studio, so an old build
cannot be updated file by file. The launcher says so (a notice on the home screen and in **Performance**) and, when you
press **Delete and rebuild**, moves the old `build` folder aside, compiles the game from scratch and deletes the old one
once the new build works. If the new build fails, the old one is put back. You can keep playing the current version
meanwhile. Your saves and the memory card (`game/Mcd001.ps2`, `game/mc0/`) are outside the `build` folder and are never
touched. With the installer script alone, `install.cmd` reconfigures the old folder for clang by itself.

## Removing the installation

Nothing is installed outside the folders above. Delete the installation folder (or, in a checkout, `game/`,
`build/`, `build-gen/`, `vu1_captures/`, `build-rust/`) and, for the launcher, its data folder
(`%APPDATA%\GOW_PC\Launcher` or `~/.local/share/GOW_PC/Launcher`). **Copy `Mcd001.ps2` out before deleting `game/`**:
that file is your save.
