# Troubleshooting

Most problems come down to one of three things: the wrong disc, a missing build tool, or a laptop that isn't giving
the VU1 thread enough CPU. This page is organized by symptom. If you end up opening an issue, the last section says
what to attach.

## Where the logs are

| Log | Where |
|---|---|
| Last game run (stdout + stderr) | Launcher: **Other → Last game log**. File: `%APPDATA%\GOW_PC\Launcher\logs\last.log` (`~/.local/share/GOW_PC/Launcher/logs/last.log`), previous run in `previous.log` |
| Installation | `logs/install.log` in the launcher's data folder |
| Optimization | `logs/optimize.log` in the launcher's data folder |
| Recompilation | `build-gen/recomp.log` |
| Optimization when run by an installer | `build-gen/optimize.log` |

The game log is plain text. Lines starting with `[gow-`, `[gs`, `[vu1` and `[iop` are the useful ones.

## Installation

| Symptom | Cause / fix |
|---|---|
| "God of War ISO not found" | Put the ISO in `game/` or pass the path (`install.cmd -Iso ...`, `./install.sh "/path/file.iso"`). Only the **USA** disc (`SCUS-97399`) works |
| The wizard rejects the ISO | It must be the USA disc (`SCUS-97399`) or a disc modified without changing the game's code (a translation, an undub, a data mod). Other regions and demos are named but not supported yet; discs whose executable changed are refused. An `.xdelta` patch is applied in the wizard itself: choose the original ISO and tick **Apply a patch (.xdelta)** (see [Modified discs, patches and other editions](INSTALL.md#modified-discs-patches-and-other-editions)) |
| The wizard says a `.chd` cannot be read | Only complete CHDs of version 5 (every `chdman` since 2012) are read. An older CHD, or a delta CHD that needs its parent, can be turned into a complete one with `chdman copy -i old.chd -o new.chd`, or extracted to an ISO with `chdman extractcd` / `extractdvd` and chosen as an ISO. "Damaged" means a block does not match its checksum: copy or download the CHD again |
| The wizard refuses a `.xdelta` patch | The message says why: made for another ISO (it needs the original USA disc), damaged (download it again), made with secondary compression (ask for one made with `xdelta3 -S none`), or it changes the game's executable (code patches such as widescreen hacks cannot run on the port). The original ISO is never changed, and nothing is left behind |
| "Visual Studio is missing" | Since 0.3.0 the launcher builds with a portable clang and does not need Visual Studio. From a checkout without prebuilt tools (`bin\ps2_recomp.exe`, `bin\ps2-tools.exe`), or with `-Compiler msvc`, it still does: tick the Build Tools component in the wizard, or run `install.cmd -InstallTools`. It needs administrator permission and 15–30 minutes |
| `The Visual Studio installer ended with code 1` | Another Visual Studio Installer window was open, or Windows has an installation pending. Close the installer window, restart Windows if it asks, and try again. If you already installed Visual Studio yourself with "Desktop development with C++", do not tick the Build Tools component: run the installation without it. The error lines of the installer's own logs (`%TEMP%\dd_*.log`) are copied into `install.log`. If it ends within seconds, check the `Visual Studio found:` lines near it: a Visual Studio installed **without** the C++ compiler is the usual reason, and the message then says to open that Visual Studio's Installer, choose Modify and tick "Desktop development with C++" |
| `A Visual Studio installer is open or still working` | The Visual Studio Installer was still installing (or just open) when the installation started: its `vcvars64.bat` appears long before the compiler (about 14 minutes into a clean Visual Studio 2026 install). Wait until the Installer window says it finished, restart Windows if it asks, and run the installation again |
| `cl not found after loading vcvars64.bat or VsDevCmd.bat` | Visual Studio is installed but the C++ compiler does not load. Open the Visual Studio Installer, choose Modify, and check that "Desktop development with C++" has an "MSVC build tools" component and a Windows SDK ticked; finish any installation that is still running. `install.log` lists every Visual Studio found, what each loader printed and which MSVC toolsets exist: attach it to the issue |
| `The C++ compiler is installed ... but the Windows SDK is not` (or, on 0.2.5 and older, `cmake ended with code 1` after `RC Pass 1: ... no such file or directory` and `--mt=CMAKE_MT-NOTFOUND`) | The Visual Studio has the compiler but no Windows SDK, which provides `rc.exe` and `mt.exe`. Open the Visual Studio Installer, choose Modify, tick a Windows SDK (for example "Windows 11 SDK") under "Desktop development with C++" or "Individual components", install, and run the installation again |
| `error C2131: expression did not evaluate to a constant` in `ps2_vu1_native.inl` (build stops near the first runtime files) | A Visual Studio 2022 older than the build the port is developed with (MSVC 14.40, 17.10) rejected a line that newer compilers accept. Fixed in 0.2.6; on 0.2.5 or older, update the Visual Studio (the Installer's Update button) or install Visual Studio 2026, and run the installation again |
| `Cannot run a document in the middle of a pipeline: ...\usr\bin\cmd` at `==> Checking tools` | devkitPro or MSYS2 has its `usr\bin` folder before `C:\Windows\System32` in `PATH`, and the installer picked their `cmd` instead of Windows' own. Fixed in 0.2.7 (the installer now calls the interpreter through `%ComSpec%`). Until then, in `%APPDATA%\GOW_PC\Launcher\kit\install.ps1` replace the line `cmd /c $cmdFile \| ForEach-Object {` with `& $env:ComSpec /c $cmdFile \| ForEach-Object {` and run the installation again |
| `cmake ended with code 1` and `The source directory ".../build" does not exist` | A CMake older than 3.21 was picked up, usually the one bundled with Visual Studio 2017 or early 2019. Update the launcher (0.2.0 or newer skips it and downloads a portable CMake) and run the installer again. Read the `Visual Studio:` line and the `cl`, `cmake` and `ninja` lines under it, near the top of the log: they show which tools were used |
| The build fails near the end with `RC1107` on `gow.rc.res` | The CMake in `PATH` comes from another toolchain and lacks `cmcldeps.exe`. Launcher 0.1.0 used that CMake. Update the launcher: the installer now skips a CMake without `cmcldeps.exe` and uses Visual Studio's, or downloads a portable one. If the log has no `Visual Studio:` line, you are still running the old installer |
| ``cmake: .../usr/lib/libcrypto.so.3: version `OPENSSL_3.x.0' not found (required by /usr/lib/libssl.so.3)`` (Linux, AppImage) | The AppImage's bundled libraries leaked into the tools the wizard runs, so the system `cmake` mixed the AppImage's older `libcrypto` with your newer `libssl`. Fixed: the launcher now removes its own library path from everything it starts (the installer, `cmake`, the game). Update the launcher; as a workaround on an older one, start it with `env -u LD_LIBRARY_PATH ./<file>.AppImage` or run `./install.sh` from a checkout |
| `--max-memory-mb needs a positive integer` (Linux, the VU1 step) | Fixed in 0.2.4: the generator now accepts `0` (unlimited), which is what the build passes by default. On 0.2.3 or older set `PS2X_VU1_MAX_MEMORY_MB` to `4096` in the build's CMake cache and run the installation again |
| The wizard says the processor does not support SSE4.1 and the first step cannot advance | The Windows build uses SSE4.1 instructions throughout, so the game cannot run on a CPU older than Intel Core 2 "Penryn" (2008) or AMD "Bulldozer" (2011), and the wizard blocks the installation instead of building for an hour first. Some virtual machines hide the feature from the guest: enable the host's CPU passthrough option (for example "host" CPU type) and open the launcher again. If the CPU really has SSE4.1 and only the guest hides it, `install.cmd -SkipCpuCheck` skips the check (the wizard has no such switch) |
| The wizard lists missing kit files (`install.ps1`, `bin/ps2-tools.exe`...) and does not go past the location step | An antivirus removed files from the launcher's data folder (`%APPDATA%\GOW_PC\Launcher\kit`). The launcher restores them from its own bundled copy each time it opens, so close it and open it again; the repair is noted in `logs/kit.log`. If they stay missing, extract the portable zip again with its `kit` folder, or add the data folder to the antivirus exclusions |
| The failure screen says a download failed | The installer could not fetch Visual Studio Build Tools, Git or CMake. Check the Internet connection and any firewall or antivirus, then try again: finished steps are skipped |
| The failure screen says no usable CMake was found | See the two CMake rows above: update the launcher, or install Visual Studio 2022 Build Tools |
| A step fails with a path or "file not found" error, and `install.log` starts with a warning about the folder | The install folder has spaces, accents or a long path (the build writes deep paths). Install to a short folder without spaces, for example `D:\GoW`: it fixed a failing MinGit and recompile step in issue #15 |
| "The path cannot contain accents" | Choose an ASCII path (the launcher enforces it; `install.ps1` alone falls back to `%SystemDrive%\PCGen` for the generated code) |
| Not enough disk space | About 20 GB while installing: the ISO and the extracted disc (~8 GB each) plus the build and the compiler (~2 GB). The wizard checks before starting; with the ISO on another drive, about 11 GB are enough |
| The build takes hours | Normal on the first run (6,400 C++ files). Reduce `-Jobs`/`--jobs` if the machine starts swapping; the linker step alone can take a long time with the MSVC toolchain |
| "CMake does not match the source" | A build folder was configured from another checkout; the installers detect it and reconfigure. If you build by hand, delete `build/CMakeCache.txt` |
| The VU1 step warned "could not apply the manifest" | Your ELF is not the retail one. The installer falls back to capturing the microcode by playing, which is slower to converge |
| Installation cancelled | Everything finished is kept; run it again and it resumes |
| `Could not set up the compiler` (0.3.0 and newer) | The portable clang (llvm-mingw, about 170 MB from github.com) could not be downloaded or unpacked; the launcher shows it as a compiler download failure. Check the Internet connection and any firewall or antivirus (some quarantine files inside the archive), then try again: the download is checked by SHA-256 and kept once it is complete. `install.cmd -Compiler msvc` builds with Visual Studio instead |
| `the disc image is damaged or is not a PS2 game disc` (0.3.0 and newer) | The ISO is cut (a download or copy that stopped early) or is not a PS2 disc: a file runs past the end of the image, a name is invalid, or the directories loop. Make the ISO again from the original disc. A cut padding file (`DUMMY...`) only gives a warning |
| `The build folder uses another compiler` (0.3.0 and newer) | The build folder kept the compiler of an older installation. Delete `build\CMakeCache.txt` in the installation folder and run the installation again; the downloaded libraries in `build\_deps` are kept |

## Starting the game

| Symptom | Cause / fix |
|---|---|
| Launcher says the game files are missing | **Files → Find files automatically**, or set `SCUS_973.99` and `gow-2005` by hand |
| "ELF does not match this build" | The runner was built for another ELF (or the file changed). Rebuild, or set the runner the build produced |
| Illegal instruction, or the game closes at startup with no message, on an old CPU | The game needs a CPU with SSE4.1 (Intel Core 2 "Penryn", 2008, or newer; AMD "Bulldozer", 2011, or newer). The installer checks it, so this usually means the check was skipped or the game was copied to another PC. There is no workaround: the instructions are used by the generated code itself |
| The error dialog says the graphics driver could not provide the OpenGL context (the log has `core context unavailable`) | Update the graphics driver. A remote desktop session and the Microsoft Basic Display Adapter cannot run the game; play on the PC itself with the GPU's own driver |
| The game closes with code `-1073741515` (`0xC0000135`) or `-1073741701` (`0xC000007B`) | Windows could not find a DLL the game needs (`0xC0000135`), or a DLL or the game itself is damaged or built for another kind of PC (`0xC000007B`). Run the installation again on this PC, and check that no antivirus removed files from the game folder |
| Window opens and closes instantly | Read `logs/last.log`; usually a missing renderer feature (below) or a missing game data file |
| Dark window saying "The graphics renderer could not start" | The GPU or its driver gives less than OpenGL 4.3 (or rejected the shaders). Update the graphics driver or use a different GPU; the details are in `last.log` (`[gs-hw] GL unavailable` or `shader error`). |
| Black screen, crash or "could not create the GL context" on an old GPU | The renderer needs **OpenGL 4.3 or newer** (it tries 4.6, 4.5, 4.4 and then 4.3, and `last.log` has a `[gs-gl] OpenGL 4.x core context` line with the one it got). Update the driver; on laptops use the dedicated GPU; on Linux make sure the proprietary/Mesa driver for your card is actually in use |
| Windows SmartScreen warning | The launcher is not code-signed yet: "More info → Run anyway". Verify you downloaded it from the project's Releases page |
| Third-party antivirus blocks the build | The build runs `cl.exe`, `ninja` and the recompiler; add the installation folder to its exclusions |

## Performance and audio

| Symptom | Cause / fix |
|---|---|
| Runs in slow motion | Check `speed=` in the log (`GOW_DELTA_DIAG=1`). Usually the CPU: close background builds, use the performance power profile, and run the optimization pass |
| Slow in combat only | VU1-bound. Run `-Optimize`/`--optimize` again after playing new areas, or let the launcher's background optimization finish |
| Slow at 3× and above | GPU-bound, as expected. Go back to 2×. The launcher's *fluency fallback* is what keeps heavy scenes at 60 |
| Audio crackles or cuts | Usually the GS running inline with the EE: the launcher sets `PS2X_GS_WORKER=1`; if you start the game by hand, export it. Also check that the CPU is not throttling |
| Speech or music slightly slower or faster for a moment | Expected: when the frame rate drops, the audio is stretched (pitch kept, at most 15 % slower) instead of cutting to silence, and it returns to normal speed once the buffer refills. `GOW_AUDIO_STRETCH=0` turns this off for comparison; `GOW_AUDIO_STATS=1` logs the buffer level and tempo |
| Sound too quiet compared with 0.2.x | Since 0.3.0 the game's sound is 4 dB lower, so fights no longer sound blown out (the mix went over full scale and the limiter cut it in half). Raise the system volume, or set `GOW_AUDIO_GAIN_DB=0` for the old level |
| No audio device | `GOW_AUDIO_DEVICE=default` picks the system default; set another device name if you have several |
| Window stutters while the FPS number is fine | Disable VSync (not recommended for this game); with the 120 FPS limit, go back to 60 |
| Something timed feels off at 120 FPS (a QTE, a combo, a jump) | Actions timed in frames are not fully verified at 120: set **Performance → FPS limit** back to 60 and report the scene |

## Saves and the memory card

| Symptom | Cause / fix |
|---|---|
| "No MEMORY CARD found" | The card image is missing: `Mcd001.ps2` next to `SCUS_973.99` (in `game/`). Copy a PCSX2 card there, or let the game create one |
| My PCSX2 saves are not listed | The file must be a PCSX2 card image named `Mcd001.ps2` next to the executable, or point `GOW_MC0` at it |
| The save does not appear in PCSX2 | Copy the whole `Mcd001.ps2` out of `game/`; do not copy parts of it |
| **The game hangs right after saving** | A known open problem on Windows. Note the area and what you did, then attach `logs/last.log` and the `gow_hang_*.sampling` files next to it. If the hang report has `[gow-hang] animation step loop` and `animation controller` lines, those values are what is needed to fix it |
| **The game closes or stops responding and `last.log` just ends** | Since 0.3.4 a crash leaves a `[gow-crash]` line at the end of `last.log` (the exception code and a place such as `gow-2005.exe+0x...`), after `[gow-trace]` lines with the last game branches. Use **Mark a problem** with what you were doing and attach the report. A log that still ends with neither line usually means the process was closed from outside (Task Manager, the PC shutting down); say so in the report if you did |
| **The picture freezes and `last.log` says `the game has not entered the frame function for 5 s`** | Two different things trigger that line. During a cutscene it is a false alarm (the movie player does not use the frame function; fixed in 0.2.9: no line while a movie plays). Outside a movie it is a real freeze: the lines `[gow-hang] gs_done_flag=... csr=... imr=...` that follow say what the game waits for (a `gs_done_flag` of 1 with `csr` bit 1 clear or set tells a lost FINISH from an undelivered interrupt) and `[gow-gs-irq]` lines say when the port had to deliver the GS interrupt again. Attach `logs/last.log` and the `gow_hang_*.sampling` files next to it |
| **`last.log` has `[gow-badcall]` lines, or the picture freezes after a `[guest-branch:missing-target]` line** | The game called a function through a bad pointer. Before 0.3.4 that froze the picture for good (`[gow-trace] main thread dormant`); since 0.3.4 the call is skipped and the game goes on, possibly with something missing on screen. Use **Mark a problem**, say where in the game it happened and attach the report: the `[gow-badcall]` and `[gow-trace]` lines are what is needed to fix the cause |
| **Blocky or stale textures after saving, until the game is restarted** | `last.log` shows `[vif1] unhandled VIFcode` lines right after `[memory card] backup kept`: a VU1 command stream lost its alignment, and texture uploads queued behind it could stay there. Since 0.3.4 they are released (`[vif1] stream fault with PATH3 masked`), and the first faults print `[vif1] history` and `[vif1] word` lines, which are what is needed to fix the cause. Attach `logs/last.log` |
| Save does not survive a reinstall | `game/` is deleted on a clean reinstall. Back up `Mcd001.ps2` |

## Controls and features

| Symptom | Cause / fix |
|---|---|
| A key does nothing | It may be bound to two actions, or be one of the reserved keys (F1/F2/F11). **Controls** marks conflicts |
| Ctrl casts magic instead of dodging | Ctrl is R2 while standing still; hold a movement key to dodge |
| F2 does not open the cheat menu | The game window must have focus. With a pad, only the captured button/combo opens it |
| No rumble | Vibration is implemented on **Windows only** |
| Gamepad not detected | Only controller port 1 is connected. Reconnect the pad before starting the game |
| A pad behind a wireless dongle does nothing (Linux) | The dongle shows up as several devices (media keys, keyboard, the pad) and the pad may have no controller mapping, so every button reads as released. Since 0.2.4 a generic Xbox-class pad (`Generic X-Box pad`) gets the standard Xbox layout, and the log has a line `[pad] joystick N "..." has no SDL mapping: using the Xbox (xpad) layout`. Press any button on the pad once: the game switches to the device that sends input. Since 0.2.5 the **Controls** tab lists the devices the game saw (start the game once with the dongle connected and close it), lets you pick the one to read, and, for a pad the system has no layout for, lets you map its buttons by hand. A pad of another kind that still does nothing needs the `GOW_PAD_DIAG=1` log |
| Pad (and keyboard) do nothing in the game | The keyboard is read together with any gamepad (before 0.2.1, any listed gamepad switched it off). If a pad does nothing, close Steam, DS4Windows and similar tools, then try it over USB; F1/F2 are window features and still work. To report it, set `GOW_PAD_DIAG=1` before starting the launcher, press buttons for ~10 s and attach `logs/last.log` (the `[gow-paddiag]` lines) |
| Target lock does nothing | Alt must be pressed and released alone, with the window focused, and there must be an enemy within range |
| Custom textures do nothing | **Textures → Custom textures** must be on, the files must be in `textures/replacements`, and the game must be restarted |
| Achievements not recognized | The ISO must be the original (or an unmodified `SCUS_973.99`). Hardcore mode turns cheats off by design |
| Achievements password not saved on Linux | Not implemented there; the credential store is Windows-only |

## Reset and start over

- **Launcher settings**: **Other → Restore defaults** (graphics, performance, controls and cheats; paths are kept).
- **Settings file**: delete `settings.json` in the launcher data folder.
- **Game files and build**: delete `game/`, `build/`, `build-gen/` and `vu1_captures/` (back up `Mcd001.ps2` first) and
  run the installer again.
- **Recompile only**: run the installer again without deleting anything; finished steps are skipped.
- **Play without the launcher** (Linux): `./run_gow.sh`, which uses the same environment as the launcher's defaults.

## What to include in a bug report

**The easy way:** in the launcher open **Other**, press **Export report** and attach the ZIP it saves to the issue. It
holds the logs, your settings, the environment the game was started with and details of your PC (system, processor,
memory, graphics driver, installation facts), with your user name, computer name and folders replaced by placeholders. It
never contains game files, saves, captures or textures. The failure dialog has the same button. **Mark a problem**
(Other tab or the failure dialog) adds a short note to the next report, with how far into the game it was. Open the ZIP
and look through it before attaching: the removal is by text match, and `README.txt` inside lists what it cannot catch.

If you cannot use the launcher, or for a build problem, give:

1. **Platform and version**: OS, GPU and driver, launcher version (About) or commit hash.
2. **How it was built**: the launcher wizard, `install.cmd`/`install.sh`, and whether `-FastDev` was used.
3. **What you did**: the area, whether it was a save, a movie, a menu; if it is performance, the scene and the settings.
4. **The log**: `logs/last.log` (or `build-gen/recomp.log` for build problems).
5. **The game-speed line**: `speed=` from the log, if the complaint is performance.

Do **not** attach game files, RAM/VRAM dumps or captured VU1 microcode: they are game data and must stay on your
machine.
