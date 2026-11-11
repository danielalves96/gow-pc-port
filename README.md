<div align="center">

<img src="https://i.ibb.co/FLBHkcHL/god-of-war-native-pc-port-banner.png" alt="God of War: native PC port" width="100%">

**English** · [Português (Brasil)](README.pt-BR.md)

*God of War* (PlayStation 2, NTSC-U `SCUS-97399`) **statically recompiled** into native C++, for Windows and, experimentally, Linux.

[![Static recompilation](https://img.shields.io/badge/static%20recompilation-not%20an%20emulator-2ea44f)](#not-an-emulator)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20x64-0078D6)](#platform-support)
[![Language](https://img.shields.io/badge/C%2B%2B-20%20%2B%20Rust-00599C)](docs/dev/ARCHITECTURE.md)
[![Renderer](https://img.shields.io/badge/renderer-OpenGL%204.6-5586A4)](docs/dev/RENDERING.md)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue)](LICENSE)
[![Discord](https://img.shields.io/badge/Discord-Join%20the%20community-5865F2?logo=discord&logoColor=white)](https://discord.gg/QQmjMx7hz)

</div>

---

> [!TIP]
> **Download:** the program is on the [**Releases** page](https://github.com/danielalves96/gow-pc-port/releases). Get the
> [**latest release**](https://github.com/danielalves96/gow-pc-port/releases/latest): on Windows the installer
> (`..._x64-setup.exe`) or the portable zip, on Linux the `.deb` or the AppImage. The green **Code** button (and
> "Source code" in a release) only gives you the sources, with no installer: that is for building it yourself. You also
> need your own copy of the game, see [Installing](#installing).

> [!IMPORTANT]
> **This is not an emulator.** It is a **static recompilation (recomp)**: the game's MIPS executable is translated to
> C++ ahead of time and compiled into a native program. There is **no MIPS interpreter, no emulated PS2 hardware and no
> BIOS** at run time. [`docs/dev/ARCHITECTURE.md`](docs/dev/ARCHITECTURE.md) shows where that line falls.

> [!IMPORTANT]
> **This repository ships no game data.** No ISO, no `SCUS_973.99`, no IRX modules, no `.PAK` files and no code
> generated from them. You need your own legal copy of the **USA** release of *God of War* for the PlayStation 2
> (`SCUS-97399`). The installer builds everything on your machine from your own disc, and nothing derived from the game
> ever leaves it.

## The short version

**Static recompilation, not emulation.** A recompiler written in Rust **translates the game's MIPS code to C++**, which
is then compiled into a normal PC program. The PlayStation 2 hardware around it (graphics synthesizer, vector unit, I/O
processor, sound, pad, memory card) is implemented natively. Nothing interprets the game's code at run time.

On top of that you get the original 60 Hz game logic (or a 120 FPS limit, the highest, at the same speed), an
OpenGL renderer with 1×–8× internal resolution, native movies
with sound, a launcher with settings, keyboard and mouse support, a cheat menu, target lock, widescreen, custom texture
packs and RetroAchievements.

## Not an emulator

| | An emulator (e.g. PCSX2) | This port |
|---|---|---|
| How the game runs | Executes the original MIPS binary instruction by instruction | **Translates it to C++ once**, then compiles that into `gow-2005` |
| PS2 hardware | Emulated: CPU, GS, VU, IOP, DMA, timers… | **Native services**: OpenGL GS, VU1 microcode compiled ahead of time, the game's IRX modules compiled to C++ by the optimization pass |
| BIOS | Requires a PS2 BIOS dump | **None.** Kernel and library services are implemented in the runtime |
| Your disc | Mounted or emulated on every run | Read once at install time; afterwards the extracted files are read by native disc code |
| What you run | A console application inside a virtual machine | An ordinary native executable, `gow-2005` |

What that means in practice: the game's own code and the hardware around it are **compiled C++**, not something
replayed. There are no emulation accuracy hacks here (frame hacks, cycle hacks, per-game patches). The port runs the
game's own recompiled code and replaces the libraries that code calls with native ones.

## Features

- **Native renderer**: OpenGL 4.6, GPU render targets, internal resolution 1×–8×, bicubic (with sharpening),
  bilinear or pixel upscaling, FXAA, 480p progressive mode, brightness/contrast/gamma/saturation adjustment.
- **Widescreen (Hor+)**: 16:9, 16:10, 21:9 (experimental) and 32:9 (very experimental: stretched HUD and menus) using PCSX2's patch. Switch it off for the game's original 4:3.
- **Movies**: MPEG-2 decoded natively, with sound, including the game's own widescreen versions.
- **Controls**: remappable keyboard and mouse, any gamepad, PS2 or modern layout, per-button swaps, vibration on
  Windows and Linux.
- **Quality of life**: cheat menu (**F2**, nine cheats), target lock (**Alt**), agile combat, overlay with FPS, GPU, CPU and RAM (each one optional), fullscreen.
- **Saves**: a PCSX2-compatible `Mcd001.ps2` memory card. Cards move between the two in either direction.
- **Custom textures**: PCSX2 replacement packs work, and the launcher can download the community pack.
- **RetroAchievements**: sign-in, in-game notifications and a hardcore mode that disables cheats.
- **Launcher**: eight tabs, a setup wizard, background optimization, English and Brazilian Portuguese.

## Platform support

| | Windows | Linux |
|---|---|---|
| Installer | `install.cmd` / `install.ps1` (14 options) | `install.sh` |
| Launcher | `God Of War - PC Launcher_<version>_x64-setup.exe` or the `..._x64-portable.zip`, with the setup wizard | `.deb` / AppImage, with the setup wizard (it drives `install.sh`) |
| Status | **Primary target** | **Experimental** |

> [!WARNING]
> **Linux is experimental.** The Linux path is in the repository (`install.sh`, the GLFW/GL presentation path,
> `run_gow.sh`, the `.deb`/AppImage packaging), but it hasn't been validated to the same level: no CI job builds or
> tests the runtime on any platform, the launcher's setup wizard drives `install.sh` on Linux but has never run on a real
> Linux machine, and recent Linux builds haven't been measured. Treat it as prepared, not supported.
> [`docs/STATUS.md`](docs/STATUS.md) has the details, area by area.

## Contents

- [The short version](#the-short-version)
- [Not an emulator](#not-an-emulator)
- [Features](#features)
- [Platform support](#platform-support)
- [What you need](#what-you-need)
- [Installing](#installing)
- [Playing](#playing)
- [Performance](#performance)
- [Repository layout](#repository-layout)
- [How it works](#how-it-works)
- [Why the build is slow and big, and why the PS2 is hard](#why-the-build-is-slow-and-big-and-why-the-ps2-is-hard)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [Credits](#credits)
- [License and legal](#license-and-legal)

## What you need

- **Your own *God of War* (USA, `SCUS-97399`) ISO**, about 8 GB. A disc modified without changing the game's code (a
  translation, an undub, a data mod) also works, and an `.xdelta` patch can be applied to the original in the wizard.
  A `.chd` image (from `chdman`) works too: the wizard converts it to an ISO next to it.
- **~20 GB free** while installing (your ISO, the extracted disc and the build); about 11 GB stay once the ISO is gone.
- **64-bit x86 CPU with SSE4.1** (Intel Core 2 "Penryn", 2008, or newer; AMD "Bulldozer", 2011, or newer; the AMD Phenom II does not have it), 4 cores or more recommended. AVX2 is *not* required.
- **OpenGL 4.6** GPU (any recent NVIDIA/AMD, or a recent Intel/AMD iGPU).
- **8 GB RAM** (16 GB while building is comfortable).
- **Windows 10/11 x64** or a recent Linux (Arch/CachyOS, Ubuntu 24.04+, Fedora 40+); Linux is experimental.
- **About 10–20 minutes** for the first build on Windows (10 on a 6-core Intel Core i5-12400F; longer on 4 cores or
  older CPUs). On Linux, which still compiles with g++, it can take 30–90 minutes.

## Installing

**Windows: the launcher.** Download the installer from the Releases page, run it, and let the setup wizard do the work.
It finds and verifies your ISO, downloads a portable C++ compiler (no Visual Studio needed since 0.3.0), extracts the disc, recompiles the
game and builds it. Then press **PLAY**.

> **Windows may warn about an "unknown publisher".** The launcher's installer is not code-signed yet, so SmartScreen
> shows "Windows protected your PC". Click **More info**, then **Run anyway**. The installer contains no game files and
> its sources are in this repository.
>
> **Prefer not to run an installer?** The same release has a **portable zip** (`..._x64-portable.zip`): unzip it, open
> `God Of War - PC Launcher.exe` and the wizard does the rest. Unblock the zip before unzipping (right-click,
> Properties, Unblock) or Windows shows the same warning for the `.exe`. It needs the WebView2 runtime, which Windows
> 11 and updated Windows 10 already have. Every release also lists the SHA-256 of its files in `SHA256SUMS-*.txt`.

**Linux: the script (experimental).**

```bash
./install.sh --install-deps "/path/to/God of War.iso"   # or put the ISO in game/ and run ./install.sh
./run_gow.sh                                            # play
```

The script checks for CMake, Ninja, a C++ compiler, Git and the GL, X11, Wayland and ALSA development files, and for
disk space (20 GB) and RAM (4 GB at least, 8 GB recommended) before it starts. On Arch, Debian/Ubuntu and Fedora (and
relatives) `--install-deps` installs what is missing through `sudo`; without the flag it asks in a terminal and
otherwise prints the command. On any other distribution it lists what to install. Rust is not needed when the
launcher's kit supplies the tools. The launcher's setup wizard does the same on Linux, but you run the package command
yourself, because it needs your password.

Both installers accept options (prebuilt tools, destination folder, job count, optimization). Every flag is in
[`docs/INSTALL.md`](docs/INSTALL.md).

> The first build compiles about 6,400 generated C++ files. Later updates only rebuild what changed.

## Playing

Keyboard and mouse work out of the box and are fully remappable in the launcher:

| | | | |
|---|---|---|---|
| Move | **W A S D** | Light / heavy attack | **Left / right click** |
| Jump | **Space** | Grab | **F** |
| Block | **Q** | Action (R1) | **E** |
| Magic (L2) | **Left Shift** | Interact (R2) | **Ctrl** while standing |
| Rage of the Gods | **V** | Blade of Artemis | **Middle click** |
| Magic 1–4 | **1 2 3 4** | D-pad / camera | **Arrows** / **I J K L** |
| Cheat menu | **F2** | Fullscreen | **F11** |
| Target lock | **Alt** (mouse switches target) | Developer panel | **F1** |

Gamepads (DualShock, DualSense, Xbox, 8BitDo) are plug and play; vibration works on Windows. Saves go to an
`Mcd001.ps2` memory-card image next to the executable, which is **PCSX2-compatible in both directions**.

Everything else (the full cheat list, target lock rules, agile combat, memory card details, achievements) is in
[`docs/PLAYING.md`](docs/PLAYING.md). The launcher's tabs and options are in [`docs/LAUNCHER.md`](docs/LAUNCHER.md).

## Performance

The game runs at 60 FPS by default, the PS2's own rate, always at real-time speed: late frames are dropped, never slowed
down. The launcher's 120 FPS limit runs the game logic at that rate, smoother but never faster; the window refreshes at
the monitor's rate, at most 120. Nothing runs above 120 FPS: past it a weak PC gets no smoother, only hotter.

- **Internal resolution 2× is the sweet spot.** 3× and above are heavy and, on mid-range cards, can stall the
  presentation.
- The CPU side is dominated by the **VU1** (the vector unit). Every optimization pass converts more of it to native
  code: run `install.cmd -Optimize` / `./install.sh --optimize` after playing new areas, or let the launcher's
  background optimization finish.
- A laptop **power-saving profile can halve the frame rate**, because it clocks down the CPU the VU1 thread needs.

Measured reference points and the tuning order are in [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md).

## Repository layout

```
.
├── install.sh / install.cmd / install.ps1   installers (Linux / Windows)
├── run_gow.sh                               play without the launcher (Linux)
├── RustPortPS2/
│   ├── crates/                              Rust: ps2_recomp, ps2-tools, ps2-parity, ps2-devtools, …
│   ├── native/                              C++ runtime: ps2xRuntime, ps2xIOP, ps2xTest, tools, tests
│   ├── games/scus-97399/                    recompiler configuration, function map, VU1 manifests
│   ├── third_party/rabbitizer/              vendored MIPS disassembler (MIT)
│   └── docs/                                PARITY.md (the byte-identical gate)
├── gow-launcher/                            launcher (Rust gow-core + Tauri 2 + React)
├── scripts/                                 build helpers and the CI script checker
├── docs/                                    player documentation (dev/ for contributors)
└── LICENSE                                  GPL-3.0
```

Generated code, the extracted disc and the build live in `game/`, `build-gen/`, `build/` and `vu1_captures/`, all of
which Git ignores.

## How it works

Two different programs are involved, and they never run at the same time:

| | Recompilation | Play |
|---|---|---|
| **When** | once, during the installation | every time you launch the game |
| **What runs** | `ps2_recomp` (Rust), `ps2-tools`, CMake and Ninja | `gow-2005`, the compiled game |
| **Input** | your `SCUS_973.99`, the game's recompiler configuration (`gow-port.toml`, `funcmap.csv`) and the VU1 manifest | the C++ produced above, plus the extracted disc |
| **Output** | ~6,400 C++ files and the native VU1 blocks, linked into `gow-2005` | frames, sound and saves, with no interpreter in the path |

At run time the game's own code is ordinary C++: **no MIPS interpreter and no emulator sit in the hot path**. Around it,
a runtime does the PS2 hardware's job: the GS renders with OpenGL, the VU1 runs microcode compiled ahead of time, the
IOP runs the game's own IRX modules as native code, and sound, pad and memory card are host services.

The recompiler reproduces [PS2Recomp](https://github.com/ran-j/PS2Recomp)'s output byte for byte; the C++ runtime is built on
Waffle565's *God-Of-War-Native-Port*, with the OpenGL GS backend from the SotC fork of PS2Recomp, and PCSX2 used as the
hardware reference. Architecture, build options and testing are in
[`docs/dev/`](docs/dev/README.md).

## Why the build is slow and big, and why the PS2 is hard

The numbers in [What you need](#what-you-need) (10–20 minutes on Windows, ~20 GB) are not padding. They follow from what the
installer does and from the machine being ported.

**Why the first build still takes minutes**

- The recompiler turns about 6,400 game functions into about 6,400 C++ files, roughly **129 MB of source**. Every MIPS
  instruction becomes explicit C++ (register access, memory access, branch delay slots, 128-bit multimedia operations),
  so a few bytes of machine code become many lines. The compiler then optimizes all of it in Release mode, from scratch,
  once. That is the size of a large application, not of a small project. Since 0.3.0 Windows compiles it with clang,
  which takes about 10 minutes where Visual Studio took over an hour on the same PC.
- The VU1 (the vector unit) runs microcode that is generated ahead of time as native blocks and compiled too. Without
  that step the game would interpret VU1 code and run much slower.
- Parallelism is limited by memory, not only by cores: the installers start at most one compiler process per 2 GB of RAM
  and cap the VU1 generator at 4 GB by default. A machine with many cores and little RAM builds slowly on purpose,
  instead of running out of memory.
- Changing a header that the generated code includes, or any `CMakeLists.txt`, rebuilds everything (the precompiled
  header is regenerated). Later updates rebuild only what changed.

**Why it asks for ~20 GB**

The final game is small next to what installing needs. Measured on a 0.3.0 installation: your ISO (about 8 GB), the
extracted disc (7.9 GB), the build with its object files, precompiled header, VU1 blocks and speed profile (1.3 GB), the
portable compiler (0.5 GB, Windows) and the generated C++, tools and the copy of the game you play (about 0.4 GB). That
is about 18 GB at the peak and about 11 GB once the ISO is deleted; the requirement keeps a small margin on top.

**Why the PS2 is harder to port than most consoles**

A few broad comparisons, not a ranking of difficulty in general:

| | Typical recompilation target | PlayStation 2 |
|---|---|---|
| Processors that run game code | One main CPU | Three with different instruction sets: the Emotion Engine (MIPS R5900 with its own 128-bit multimedia extensions), two vector units (VU0, VU1) and the I/O processor (a PS1-class MIPS CPU running IRX modules) |
| Graphics | A GPU driven through a command list or a fixed library | The Graphics Synthesizer with 4 MiB of embedded video memory, fed by DMA from the CPU and the VU1; there is no graphics API and games program the hardware directly |
| Microcode | None, or a few shared microcodes (e.g. Nintendo's N64 ones) | Every game ships its own VU1 programs. They are uploaded at run time, so they cannot all be found by reading the executable; they have to be captured while playing |
| Hardware access | Through an operating system or a library | Games talk to registers, DMA and timers directly, so the runtime has to reproduce that behavior |
| Code size | Often small (cartridge or early disc games) | *God of War* has about 6,400 functions in the main executable plus its IOP modules |

That is why this port needs a recompiler, a native GS, a VU1 generator, an IOP layer and a lot of captured data, and not
just a translator for one CPU. It is also why the installer cannot ship a ready-made binary: it would contain the game's
code, which this repository never distributes (see [License and legal](#license-and-legal)).

## Documentation

| | |
|---|---|
| [`docs/INSTALL.md`](docs/INSTALL.md) | Requirements, wizard, both installers, every option, what gets created |
| [`docs/LAUNCHER.md`](docs/LAUNCHER.md) | The launcher, tab by tab, with defaults, paths and logs |
| [`docs/PLAYING.md`](docs/PLAYING.md) | Controls, cheats, saves, gamepad, achievements |
| [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) | What costs what, reference points, tuning |
| [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) | Symptom-by-symptom fixes, logs, bug reports |
| [`docs/STATUS.md`](docs/STATUS.md) | What works, what does not, open problems |
| [`docs/TEXTURES.md`](docs/TEXTURES.md) | Texture packs: install and authoring |
| [`docs/THIRD_PARTY.md`](docs/THIRD_PARTY.md) | Upstream projects, libraries, licenses |
| [`docs/dev/`](docs/dev/README.md) | Architecture, build, environment variables, rendering, audio, diagnostics |

## Contributing

Contributions are welcome: bug reports (with a log), fixes and documentation. Read
[`docs/dev/README.md`](docs/dev/README.md) and [`docs/dev/BUILD.md`](docs/dev/BUILD.md) first. The short version:

- Rust builds the tools, C++ runs the game. Never edit generated files by hand.
- Changes to code taken from the upstream projects carry a `// GOW-Port:` comment.
- `cargo test --workspace` must pass, `cargo clippy … -D warnings` must stay clean, and the parity gate must stay
  byte-identical.
- Never commit anything derived from the game.

## Credits

This port stands on other people's work, above all
[PS2Recomp](https://github.com/ran-j/PS2Recomp),
[KIexster's God of War recompilation](https://github.com/KIexster/god-of-war-recomp),
[Waffle565's native port](https://github.com/Waffle565/God-Of-War-Native-Port) and
[PCSX2](https://github.com/PCSX2/pcsx2). The full inventory, including what comes from where and under which license,
is in [`docs/THIRD_PARTY.md`](docs/THIRD_PARTY.md).

<table>
  <tr>
    <td >
      <a href="https://github.com/danielalves96"><img src="https://avatars.githubusercontent.com/u/62755605?s=120&v=4" width="120" alt="danielalves96"/><br/><b>danielalves96</b></a><br/>
      Recompiler, runtime integration, PC features, launcher, packaging
    </td>
  </tr>
</table>

## License and legal

The code in this repository is **GPL-3.0** (see [`LICENSE`](LICENSE)), like the projects it builds on. Vendored
libraries keep their own licenses, listed in [`docs/THIRD_PARTY.md`](docs/THIRD_PARTY.md).

*God of War* © Sony Interactive Entertainment / Santa Monica Studio. This project is not affiliated with or endorsed by
Sony, and distributes no game content. **Own the game.**
