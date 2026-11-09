# The launcher

*God Of War - PC Launcher* is the front end of the port: it installs the game, stores your settings and starts it with
the right environment. It is a native application (Rust + Tauri + React), not a wrapper around a command line. This
page describes what every tab does and where the launcher keeps its files.

If you just want to play, set **Graphics → render resolution 2×** and **Performance → FPS limit 60**, then press
**PLAY**.

## Tabs

### Graphics

| Control | Default | What it does |
|---|---|---|
| Display mode | Fullscreen | Fullscreen or windowed. **F11** toggles it in game |
| Widescreen (Hor+) | On | PCSX2's widescreen patch: widens the camera to the sides at the chosen aspect. Off gives you the game's original 4:3 picture, with side bars in a wide window |
| Aspect ratio | Your display's | 4:3, 16:9, 16:10, 21:9 (ultrawide, experimental) or 32:9 (super ultrawide, very experimental). 32:9 is picked by default on displays such as 5120×1440 and 3840×1080. At 32:9 the HUD is stretched across the whole width and a higher internal resolution (3× or more) keeps the image sharp |
| Resolution | Native | Fullscreen always uses the display's native mode; this is the initial window size |
| Graphics preset | Balanced | **Performance** (1×, Bilinear, no FXAA), **Balanced** (2×, Bilinear, FXAA) or **Quality** (3×, Bicubic at 0.8, FXAA). It sets only render resolution, upscaling, sharpness and FXAA, and shows **Custom** as soon as you change one of them by hand. It changes how heavy the picture is for your graphics card, not the speed of the game itself |
| Render resolution | 2× | Internal render scale, 1× to 8×. 2× is recommended: **from 3× up the game gets heavy**, and 4×–8× need a powerful GPU |
| Upscaling | Bilinear (soft) | How the image is fitted to the screen: **Bicubic (sharp)** (bicubic + contrast-adaptive sharpening), **Bilinear (soft)** or **Pixel**. The "Pixel" option currently resolves to a bilinear stretch, not nearest-neighbour. If the render is larger than the screen it is downsampled by area averaging |
| Sharpness | 0.8 | Only for Bicubic: how much contrast-adaptive sharpening is applied |
| Anti-aliasing (FXAA) | On | Smooths jagged edges before upscaling |
| Progressive scan (480p) | On | The game's sharp 480p mode. Off uses its interlaced mode with the anti-flicker filter (softer) |
| Image adjustment: Brightness, Contrast, Gamma, Saturation | 50 | PCSX2-style Shade Boost on the final picture, 0 to 100, where 50 leaves the image exactly as the game draws it. Gamma above 50 brightens the mid-tones without washing out black, which helps the dark scenes on PC monitors. **Reset** puts all four back at 50. It runs in the same pass as FXAA and upscaling, so it costs practically nothing |

### Textures

| Control | Default | What it does |
|---|---|---|
| Custom textures | Off | Draws PCSX2-format replacement textures from `textures/replacements` |
| Load textures asynchronously | On | Reads replacements in the background and keeps the game's own texture until each is ready. Off waits for every file (each new texture may pause the game briefly) |
| Textures folder → Open | - | Opens the folder where a pack goes |
| Texture pack | - | Downloads and installs the community pack. The card disappears once a pack is installed; **it replaces whatever was in the folder**. While it installs, the other sections, PLAY and these options are off and closing the window asks first |

A pack applies the next time the game starts. [`TEXTURES.md`](TEXTURES.md) explains the format and the file names.

### Performance

| Control | Default | What it does |
|---|---|---|
| Graphics card | Automatic | On Windows: Power saving (integrated) or High performance (dedicated); on Linux the launcher sets the PRIME variables. The game always renders on the card that drives the window, so a dedicated GPU is much faster |
| FPS limit | 60 | The game's real frame rate. 60 is the native rate, 120 runs the game smoother and **240 is experimental**: the game logic runs at that rate with the game kept at normal speed (smoother, never faster). The window refreshes at the monitor's rate. Settings from older versions go back to 60 once |
| Vertical sync | Off | Prevents tearing; locks the presentation to the display refresh. **Not recommended**: a frame that misses a refresh waits for the next one, which shows as stutter and input lag. Leave it off unless tearing bothers you and your display has no FreeSync/G-Sync; turning it on shows a warning |
| In-game overlay: FPS counter, GPU, CPU, RAM | Off | One switch per line of the overlay in the corner of the screen: the game's real frame rate (never above the FPS limit), GPU usage, temperature and video memory, CPU usage and clock, and the RAM the game uses. Values the system does not report are not drawn. Whoever had the old FPS switch on keeps all four |
| Background optimization | On | Recompiles the parts of the game you have reached, in the background, after you close the game |
| Also while playing | **Off** | Rebuilds with the game open. It competes for the CPU and freezes the picture for tens of seconds, so leave it off unless you are debugging |
| Optimize now | - | Runs one optimization pass immediately |
| Speed profile | - | Windows, clang builds (0.3.0). **Optimize now** trains a speed profile (clang PGO) for this PC: the game opens by itself for about 4 minutes on a scripted fight (do not touch the keyboard), then the game is rebuilt with the profile, about 15 minutes in all. The launcher asks first, then locks itself (PLAY included) behind a window with the three steps until the end; it has a *Cancel* button. The row shows **Active** once the build uses a profile. The profile is made from your game on your PC and never leaves it |
| Rebuild now | - | Only shown, as a note at the top of the tab, when this launcher carries game fixes your build does not have yet (see below) |

**The speed-profile offer.** After an installation with clang, the first screen shows *Optimization available: more FPS* with
**Optimize now** and **Later**. *Later* hides the notice on this PC; the Performance tab keeps the button. The installation
itself does one build only, so the first game is ready as soon as possible, and the optimization never runs while you play.

**An interrupted optimization.** The first step builds a test version of the game that counts what the code does; it runs
several times slower and exists only for the 4 minutes of training. If the optimization stops halfway (the launcher closed,
a crash, a failed build), the launcher never opens that version: PLAY shows *FINISHING*, the first screen says *Slow test
version installed*, and the game is rebuilt the normal way by itself as soon as the launcher is open and the game closed
(with the profile, if the training had finished). *Rebuild now* on that notice starts it at once.

The launcher shows a loading spinner while it starts (finding the game can take a few seconds).

**Is the build stuck or just slow?** Every build progress bar (the installation wizard, the rebuild notice, the
Performance tab, *Optimize now*) has a **Details** button. The bar only moves when a file is done, and some steps
(generating the native VU1 code, linking the game) print nothing for up to about 30 minutes each, so it can sit still
while the build works. The details window says, live, whether the build's processes are using the processor (*Working:
using 9.4 processor cores*), the current step, the file count, how long it has been running and since it last printed
anything, the last file it finished and its last lines, with a button to open the full log (`install.log` in the wizard,
`optimize.log` otherwise). Only when nothing has used the processor or been printed for 10 minutes does it say the build
may be stuck. A download (the compiler, the portable CMake, Ninja or Git of a first installation) waits on the connection,
so it is shown as *Downloading* and never called stuck.

**Rebuilding after a launcher update.** A launcher update brings new game code (the runtime sources in its kit), but the
game you play is a build made on your PC, so the fixes do not reach it by themselves. The launcher remembers which kit each
build was made from (`build/.built-from-kit` in your installation folder, written when an installation or a rebuild
succeeds). When the kit it now carries has different runtime files or tools, it shows a note at the top of **Performance**
and one notice, once per kit version. **Rebuild now** runs the build in the background; it takes from a few minutes to about
20 minutes (a rebuild of everything takes as long as the first build), and you keep playing the current build until it finishes (the new one opens the next time you start the game).
The button waits while the game, an installation, an optimization or a texture pack is running. An update that does not
change the runtime files shows nothing, and the launcher never starts a rebuild by itself because of an update. A build
made before the launcher kept this record is assumed up to date, except when the update that introduced the record changed
the runtime files: then it asks once.

### Files

| Field | What it is |
|---|---|
| Game executable (`SCUS_973.99`) | The PS2 program from your disc. Verified by CRC; "Modified ELF · experimental build" means the file was changed |
| Disc image (ISO) | Optional: if it is empty, an `.iso` beside the game or the already-extracted files are used. The field disappears once the game data is in place |
| Port runner (`gow-2005`) | The compiled game program |
| Memory card | Read-only: the folder next to `SCUS_973.99` (`mc0`). Saves themselves live in `Mcd001.ps2` (see [`PLAYING.md`](PLAYING.md#saves-and-the-memory-card)) |

**Find files automatically** looks next to the launcher, in a repository checkout above it and in the default
installation folder. **Setup wizard** reopens the installer.

### Controls

Keyboard and mouse mapping (two keys per action), gamepad layout (Classic PS2, Modern DMC5 or **Custom**), the four
face-button swaps, **Agile combat** and **ESC closes the game**. **Custom (every button)** shows 16 rows, one per button of
the pad, each choosing the PS2 button it presses (`PC_PAD_MAP`); the first change starts from the layout in use, two buttons
may share a target (a warning is shown), Start cannot be taken away, and **Back to a layout** clears the map.
**Controller** lists the joysticks the game saw the last time it ran (name, and whether the system has a button layout for
it) and stores which one to read (`GOW_PAD_DEVICE`); Automatic leaves it to the game. The list is a file the game writes,
`pads/devices.json`, so it is empty until the game has run once with the controller connected. For a device with no button layout, **Map its
buttons** opens a dialog that learns each button from the running game: while it is open the launcher keeps `pads/probe`
fresh and the game writes the raw buttons, hats and axes that moved to `pads/live.json`. The result is stored as SDL mapping
fields (`padSdlMap`, for the chosen device only) and handed to the game as `GOW_PAD_SDL_MAPPING`. The default layout and what each action does are in
[`PLAYING.md`](PLAYING.md#keyboard-and-mouse).

Click a slot, then press the new key or click a mouse button. Right click clears it. **Esc** cancels. Keys are bound by
physical position. F1/F2/F11 are reserved and marked as such.

### Cheats

The nine cheats that start enabled (the same list is available in game with **F2**), plus the gamepad button or
two-button combo that opens the cheat menu. Details: [`PLAYING.md`](PLAYING.md#cheat-menu-f2). What you change inside the game (these cheats, the overlay lines, the image
adjustments) is read back when the game ends, so these tabs always show what the last session left
([`PLAYING.md`](PLAYING.md#settings-that-follow-you)).

### Achievements

RetroAchievements: enable, username and password, hardcore mode and in-game notifications. The password is kept in the
**Windows Credential Manager**, not in the settings file; on Linux the store does not exist yet, so the sign-in cannot
be saved. Hardcore disables all cheats. See [`PLAYING.md`](PLAYING.md#achievements-retroachievements).

### Other

Language (English / Brazilian Portuguese), **Skip FMV videos**, **Fast boot** (cuts the intro animation and may cut the
opening lines; not recommended), **Minimize the launcher while playing**, **Automatic updates** (off by default, see
[Updating the launcher](#updating-the-launcher)), shortcuts, **Last game log**, **Session report** and **Mark a problem** (see [Reports](#reports)), **Restore defaults** (resets graphics,
performance, controls and cheats; your file paths and the button layout you built for a controller the system does not know are kept) and the About box.

## The setup wizard

The wizard opens by itself the first time, or from **Files → Setup wizard**.

1. **ISO**: the launcher looks for your God of War (USA, `SCUS-97399`) ISO (Home, Downloads, Desktop, Documents,
   `~/Games` and the drive roots) and verifies it. Drag and drop or **Browse** also work. A translation that kept the USA
   code is accepted (RetroAchievements stay off with it); other regions and demos are named but not supported yet, and
   discs with changed code are refused. With the original ISO, **Apply a patch (.xdelta)** (a translation, an undub,
   a data mod) creates a patched copy next to it, with a progress bar and a Stop button, and installs that copy; the
   original is never written, and **Use the original** returns to it. A **`.chd`** image is accepted too (found,
   dropped or browsed): it is verified in place, then **Convert** writes the ISO in the CHD's folder (progress bar and
   **Stop**; the CHD is only read) and the installation uses that ISO. The patch option follows the same rule as for an
   ISO: it appears only after the conversion, and only when the disc inside the CHD is the unmodified original.
2. **Location**: where the extracted game and the build go. The path must be **ASCII only** and needs **20 GB free**.
   The default is `C:\Users\<you>\GodOfWar-PC`.
3. **Components**: shown only when something is missing. Since 0.3.0 the launcher's kit compiles the game with a portable
   clang and carries the Rust tools prebuilt, so a normal installation asks for no Visual Studio. It is listed only for a
   kit without those tools: Visual Studio 2022 Build Tools (C++, about 6 GB, needs administrator permission, 15–30
   minutes). Portable Git (about 50 MB, stays inside the install folder). A Visual
   Studio without a usable CMake (3.21 or newer, with `cmcldeps.exe` next to it) or Ninja, such as 2017, early 2019 or a
   2026 install without the CMake component, needs nothing here: the installer downloads portable CMake and Ninja into
   the install folder.
4. **Install**: one button; the progress bar has eight stages and a cancel button. **Finished steps are kept**, so if
   you cancel and try again, the install resumes instead of starting over. The full log is `logs/install.log`.

On Linux the wizard runs `install.sh` instead, with the same steps. It can't install system packages (that needs your
password), so when something is missing it lists the packages and the command for your distribution, and waits until
**Check again** finds nothing missing. The Linux path is experimental and has never run on a real Linux machine (see
[`INSTALL.md`](INSTALL.md#linux-experimental)).

> During the optimization stage the game opens by itself for a few minutes with automatic button presses. Do not touch
> the keyboard or the controller while that happens.

## Where the launcher keeps things

The data folder is `%APPDATA%\GOW_PC\Launcher` on Windows and `~/.local/share/GOW_PC/Launcher` on Linux.
`GOW_LAUNCHER_DATA_DIR=<folder>` moves everything into another folder (portable use).

| What | Where |
|---|---|
| Settings | `settings.json` in the data folder |
| Last game log (stdout + stderr) | `logs/last.log` (the previous run becomes `logs/previous.log`) |
| Installer and optimizer logs | `logs/install.log`, `logs/optimize.log` |
| Launcher update checks and installs | `logs/update.log` |
| Installation kit shipped with the launcher | `kit/` in the data folder |
| Kit repairs (a file that went missing and was restored) | `logs/kit.log` |
| Last game session (how it ended) and your problem markers | `logs/last-session.json`, `logs/markers.jsonl` |
| Joysticks the game saw last time (the Controls tab's list), and the live feed of the mapping dialog | `pads/devices.json`, `pads/probe`, `pads/live.json` |
| Window icon | `icon.png` in the data folder |
| GL shader cache | `%LOCALAPPDATA%\GOW_PC\Launcher\cache\shaders` / `~/.cache/GOW_PC/Launcher/shaders` |
| Game data, build and saves | inside your installation folder (`game/`, `build/`, `game/Mcd001.ps2`) |

The game runs with the data folder as its working directory. After a successful install the launcher keeps the runner
it verified in `<installation>/active/gow-2005`, so a rebuild in `build/` cannot break a session that is already
configured.

## What happens when you press PLAY

1. The files are checked again (runner executable, ELF present, CRC).
2. The game starts as `<gow-2005> <SCUS_973.99>` with the settings translated into environment variables, with its
   stdout and stderr going to `logs/last.log`. Among them, always `GOW_NATIVE_CHEATS_MENU=1`: the **Port Options** entry
   of the game's own pause menu, with **Cheats**, **In Game Overlay** and **Image Adjustments** inside (the overlay page switches the
   Performance tab's FPS, GPU, CPU and RAM lines on and off during the session, the image page moves its brightness, contrast, gamma and
   saturation sliders; neither is saved back to the launcher; the game leaves its stock menu alone under RetroAchievements Hardcore
   or with other game data; a `GOW_NATIVE_CHEATS_MENU=0` already in your environment turns it off).
3. The launcher can minimize itself and reappears when the game exits.
4. On exit you get the session length, or an error with the last lines of the log and a button to open it.

The launcher also pauses a background rebuild while the game is running, and resumes it afterwards.

## Installing the launcher

- **Windows**: `God Of War - PC Launcher_<version>_x64-setup.exe` from the Releases page. It installs for your user
  (no administrator permission), bundles WebView2 so it works on a clean Windows 10/11, and carries the installation
  kit. This is the supported path. The same release has a **portable zip** (the launcher and its kit, no installer);
  see [`INSTALL.md`](INSTALL.md#windows-the-portable-zip).
- **Linux (experimental)**: `.deb` or AppImage from the same Releases page. The wizard drives `install.sh` there
  (unverified on a real Linux machine); `./install.sh` from a checkout also works, and the launcher finds its files.
  Vibration is not implemented on Linux.

## When something fails

The launcher checks the kit's essential files (`install.ps1` or `install.sh`, `bin/ps2_recomp`, `bin/ps2-tools`, the
CMake and game data files the build reads) every time it opens, and restores any that is gone from the copy bundled with
it (an antivirus is the usual cause); a repair is noted in `logs/kit.log`. If the bundled copy is missing too (a portable
zip extracted without its `kit` folder), the wizard lists the missing files at the location step and does not go on.

When an installation or the game fails for a cause the launcher recognizes, a short explanation appears above the raw
error text, which stays: a download that failed, no usable CMake, a kit file that is gone, a graphics driver that could
not give the OpenGL context, and the Windows exit codes for a missing DLL (`0xC0000135`) and a bad image (`0xC000007B`).
Anything else shows only the raw text and the log button, as before.

## Reports

**Other, Export report** (and the same button in the failure dialog) saves a ZIP to `reports/` in the data folder and
opens that folder; the newest ten are kept. Attach it to a GitHub issue. It holds `report.json` (launcher version, system,
processor, memory, graphics adapters and drivers, the settings, the installation facts with the checksum of the runner and
the CRC of the game file, the joysticks the game saw, the optimizer state, the environment the game would get and what the launcher knows about the last
session), the launcher's logs (`last.log`, `previous.log`, `install.log`, `optimize.log`, `update.log`, `kit.log`; long
ones keep their first 256 KiB and their end), your markers and a `README.txt`. The contents are an allowlist of file names:
the disc image, the game file and data, saves, recorded microcode, textures, shader caches and screenshots are never
read.

Before anything is written, the installation, data, temporary, home, disc image and game folders become `<INSTALL>`,
`<DATA>`, `<TEMP>`, `<HOME>`, `<ISO_DIR>`, `<GAME>`, the user name `<USER>` and the computer name `<PC>`; the
RetroAchievements user is shown only as set or not and its password is never read. The files on disk are not changed. The
removal is by text match: it does not catch 8.3 short names or URL-encoded paths, and a word equal to your user name is
replaced too, so look through the ZIP before attaching it.

**Mark a problem** (Other tab, and the failure dialog) stores a note of up to 200 characters in `logs/markers.jsonl`, with the
time and, while the game runs, how many seconds into the session; the last 50 are kept. Do not write personal information
in them. A report can be made with the game closed, running (it is then marked partial) or after a crash: the launcher
writes `logs/last-session.json` when the game starts and when it ends.

## Updating the launcher

When GitHub has a newer *published* release of the launcher (drafts and pre-releases are ignored), a notice appears in the
title bar, the strip at the top that is the same in every tab: **Version X.Y.Z is available** and an **Update** button. It
shows nothing when you are up to date, and nothing at all when the check fails (no Internet, GitHub's rate limit): failures
only go to `logs/update.log`. The launcher checks when it opens and then at most every few hours while it stays open.

- **Installed launcher (Windows setup program, Linux AppImage)**: **Update** downloads the new version, checks its
  signature, installs it and restarts the launcher, with a progress bar. If something goes wrong the notice says so and
  offers **Retry**. Your settings, game and saves are not touched. The download is about the size of the setup program
  (around 200 MB on Windows, since it carries WebView2).
- **Portable zip, Linux `.deb`, or a release that cannot be installed in place**: the notice says the copy cannot update
  itself, and the button (**Open release page**) opens the release page, where the new zip or package is.
- **Not while something is running.** The button is disabled, with the reason beside it, while the game is running, the
  setup wizard is installing, an optimization is compiling or the texture pack is installing. It comes back by itself.
- **Automatic updates** (Other tab, off by default): with it on, the launcher checks when it opens and, if there is a newer
  version and nothing is running, installs it by itself and restarts, without you pressing anything. If it fails it does not
  try again on its own: the notice shows the error and **Retry**. With it off, you only get the notice. A portable copy
  only ever gets the notice.

How releases are made updatable is in [`dev/UPDATER.md`](dev/UPDATER.md).

The launcher is GPL-3.0; its sources are in [`gow-launcher/`](../gow-launcher).
