# Status and known limitations

What the port does today, what it does not, and how much of it has actually been exercised. This page is deliberately
blunt: you should hear about a limitation before you spend an evening fighting it.

Last reviewed: October 2026, against the code in this repository.

## Platform support

| | Windows | Linux |
|---|---|---|
| Installer (`install.ps1` / `install.sh`) | Yes | Yes |
| Builds `gow-2005` | Yes | Yes |
| Played and measured | Yes | Yes, but not re-measured recently |
| Automated coverage of the runtime or the installers | **None** | **None** |
| Launcher and setup wizard | Launcher + wizard are the main path | Launcher builds; the wizard drives `install.sh` but has never run on a real Linux machine |
| Rumble | Yes | Yes (`evdev` `FF_RUMBLE`) |
| Save-file validation | Loading and saving verified; a **hang after saving** is open (see below) | Not re-verified recently |

> [!WARNING]
> **Linux is experimental.** Everything needed is in the repository (`install.sh`, the GLFW/GL presentation path,
> `run_gow.sh`, the `.deb`/AppImage packaging and the loader/scheduler work), and the game builds and runs there. What
> is missing is proof: no CI job compiles the runtime or runs an installer on any platform, the latest Linux
> builds have not been measured, the launcher's setup wizard drives
> `install.sh` with its own arguments and `--check-deps` (unit-tested, never run on a real Linux machine), and the Windows
> installer is the only one that records `game-elf.sha256` for the launcher's fingerprint check. Don't treat a Linux
> report as a supported configuration yet.

> **Windows** is the platform the port is developed and measured on. That doesn't mean everything is proven: saves,
> long sessions and most areas are still unvalidated, and the hang after saving is open. Treat both platforms as work
> in progress, with Linux further behind.

## What works

- **The whole game is recompiled**, not emulated: about 6,400 functions of `SCUS_973.99` become C++ and run natively.
- **Rendering** is a native OpenGL renderer with GPU render targets, internal resolution 1×–8× (2× recommended),
  bicubic/bilinear/pixel scaling, FXAA, 480p progressive mode, Hor+ widescreen (4:3, 16:9, 16:10, 21:9 experimental, 32:9 very experimental).
- **Movies** play natively with their sound (MPEG-2 through libmpeg2), including the game's widescreen `.PSW` versions.
- **Sound** runs on a native sound engine fed by the game's own IOP libraries; there is no SPU2 emulation.
- **Memory card** is a PCSX2-compatible image; loading and saving work, with thumbnails and dates.
- **Input**: keyboard and mouse with a full remappable layout, gamepads with the PS2 or the modern layout, vibration on
  Windows and Linux.
- **PC features**: cheat menu (F2), target lock (Alt), agile combat, custom texture packs, RetroAchievements,
  background optimization, launcher in English and Brazilian Portuguese.
- **Verification**: the recompiler's output is compared byte for byte against PS2Recomp's C++ recompiler, and the GS,
  VU1 and EE have replay/oracle tests that do not need the game.

## Known limitations

| Area | Limitation |
|---|---|
| Frame rate | 60 FPS by default, the PS2's own rate. The 120 FPS limit (the highest: nothing runs above 120) runs the game logic at that rate with the game kept at real-time speed; actions timed in frames (QTEs, combo windows) are not fully verified there yet. Heavy scenes on integrated GPUs drop below 60 |
| Multi-threading | The VU1 thread is the CPU bottleneck in combat. The optimization pass converts more of it to native code over time. Since 0.3.2 early FINISH lets the game prepare the next frame while the VU1 draws the current one (+17% in the reference fight at the 120 FPS limit; Performance → Early FINISH turns it off) |
| Window | At 3× and above, presentation can stall behind the GPU on mid-range cards; the launcher caps presentation at 60 FPS there |
| Widescreen | With Hor+ on, the HUD and 2D screens are stretched horizontally (the PCSX2 patch does not correct them), more the wider the aspect |
| Ultrawide | 21:9 is experimental and 32:9 (5120×1440) very experimental (the HUD and menus are stretched across the whole width): the camera is widened by the same Hor+ patch, but edge pop-in, LOD and full-screen effects at those widths have not been checked in game. At 32:9 an internal resolution of 3× or more is advisable |
| Upscaling | The "Pixel" option currently behaves as a bilinear stretch (no nearest-neighbour shader) |
| Rendering corner cases | The GS backend does **not** apply the per-primitive alpha test (PABE), counts points as skipped, and does not use the game's mip levels (MIPTBP). Textures read through a CLAMP region are not replaced |
| Camera | The camera is scripted by the game and cannot be locked or moved freely; the target lock only aims attacks |
| Second controller | Port 2 is not connected |
| Custom textures | Textures read through a CLAMP region and textures sampled from render targets are not replaced; there is no texture dump |
| Blank memory card | The game's own formatting of a blank card was not verified. Since 0.3.0 the installer creates `Mcd001.ps2` already formatted (laid out like a BIOS-formatted card), so this path is only reached with a blank card copied in by hand |
| RetroAchievements | The account password is stored in the Windows Credential Manager only; on Linux the sign-in cannot be saved. Hardcore mode disables all cheats |
| Second language | Only English and Brazilian Portuguese; the game's own text is untouched |
| Disc | Only the USA release (`SCUS-97399`) and discs modified without changing its code (translations, undubs, data mods, also applied from an `.xdelta` patch in the wizard). Other regions and demos are named but not supported; discs whose executable changed are rejected |

## Open problems

- **Hang after saving (Windows):** after a save completes, a sequence of events can freeze the game. It was first
  traced to the VIF/DMA path; a 0.3.2 sample (clang build with the speed profile, early FINISH on) instead shows the game
  spinning in the animation controller update `0x116EF0`, in the loop that walks an animation's time in fixed steps
  (`0x117358..0x117384`), at the end of the save menus. Why that loop stops converging is not known yet: since this change the
  hang report prints its values (`[gow-hang] animation step loop` / `animation controller`). Not fixed; see
  [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) for what to collect.
- **Blocky picture after saving (issue #34):** right after a save, a VU1 command stream reached a reserved VIFcode in every
  frame (`[vif1] unhandled VIFcode opcode=25`). A stream that stops while PATH3 is masked never unmasks it, so texture
  uploads stayed queued until a restart, the likely cause of the picture; since 0.4.0 they are released. Not verified in
  game yet. Which command misaligns the stream is not known: the first faults now print the commands before them.
- **Character stuck after the sixth Gorgon Eye (issue #35):** reproduced by one player many times; the log just stops,
  with no hang report. Since 0.4.0 a crash leaves a `[gow-crash]` line and the log is no longer cut at exit. Not fixed.
- **Permanent freeze after a call through a bad function pointer (issues #20 and #37):** in both reports the game made
  an indirect call to an address that is not a function (#20: to 0, from the UI timeline's clip list `0x149340`, on the
  way from the docks to the first Medusa; #37: to `0x308B527B`, from the per-frame function `0x1D6CD0`, right after the
  movie of Kratos climbing Cronos, on a translated disc). The callers unwound, the main thread went dormant and the
  picture froze. Since 0.4.0:
  - #37: `0x1D6CD0` makes a virtual call on Kratos (the first field of the player controller `0x29CD58`) with no NULL
    check, and Kratos was still NULL in the first frames after the movie, so the vtable was read from address 4. The
    update is skipped while there is no Kratos (`[gow-heapguard] player update skipped`). Why Kratos is missing there
    (the experimental 120 Hz logic was on and the disc was a translation) is not known.
  - #20: the guard of `0x149340` reported nothing, so the list header itself was outside the range it checked; a list
    with such a header is now emptied (`[gow-heapguard] clone list: bad header`). What corrupts the header is not known.
  - Any other call to an address that is not a function is skipped and logged (`[gow-badcall]`, `GOW_BAD_CALL_SKIP=0`
    to turn it off) instead of freezing the game.

  None of the three was reproduced in game: the fixes come from the two logs and the disassembly.
- **Windows Release build:** the default installer configuration (no whole-program optimization, AVX2 off) has never
  been compared in game against the faster development build. Numbers measured in a development build may
  not carry over.
- **Linux:** no recent measurement, and the launcher's setup wizard has never run on a real Linux machine.
  The script is the reference way to install.
- **Portable zip:** assembled by the release workflow (the launcher, its `kit/` and a README), and the workflow checks
  that the kit is complete. It has not been run from a published release yet, so the first one is its first test. It
  does not remove the SmartScreen warning by itself: the executable is still unsigned (see [`INSTALL.md`](INSTALL.md#windows-the-portable-zip)).
- **Clang build (0.3.0):** the default Windows build moved from MSVC to a portable clang (llvm-mingw) with an optional
  speed profile trained on the player's PC. It has been built and played on one development PC only. Against the MSVC
  build it gives steadier frames (p95 17.9 → 17.0 ms in the reference fight, less than half the IOP time). A hitch every
  5 s that only the clang build had (writing a log line cost the game 50-100 ms) is fixed: the log now goes through its
  own writer thread (`GOW_LOG_DIRECT=1` restores the old path). An installation without Visual Studio on a clean PC has
  not been verified yet. `install.cmd -Compiler msvc` keeps the previous build.
- **Weak PCs (0.4.0):** measured only on a desktop CPU (Core i5-12400F) pinned to fewer cores, never on real low-end
  hardware. A dual core without SMT holds 60 FPS with no hitches (46.8 FPS with many hitches in 0.3.3), a dual core with
  SMT (i3/Pentium class) holds 60, and a dual core with cores 1.5-2x slower, approximated by taking CPU time away, runs
  at 51-55 FPS without hitches. Numbers and method in [`dev/TOASTER_PLAN.md`](dev/TOASTER_PLAN.md).
- **Coverage of areas:** no systematic play-through validation exists. Zones beyond the ones tested while developing the
  port may still show visual or audio defects.

## What this page does *not* claim

It is not a per-level compatibility list, and it is not a promise of support. If something here doesn't match your
machine, the log described in [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) is the fastest way to make the problem
actionable.
