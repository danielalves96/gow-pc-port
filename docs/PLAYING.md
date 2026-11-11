# Playing

Everything you can press, change and carry between sessions. Installation is in [`INSTALL.md`](INSTALL.md); every
launcher option is in [`LAUNCHER.md`](LAUNCHER.md).

## Keyboard and mouse

The port ships with a keyboard and mouse layout that you can **remap** key by key in **Controls**. Each action accepts
two keys (or mouse buttons). The default layout:

| Action | Default | Action | Default |
|---|---|---|---|
| Move | **W A S D** | Evade (right stick) | **Ctrl** + **W A S D** |
| Light attack (□) | **Left click** | Heavy attack (△) | **Right click** |
| Jump (✕) | **Space** | Grab (○) | **F** |
| Block / parry (L1) | **Q** | Action (R1) | **E** |
| Cast magic (L2) | **Left Shift** | Interact (R2) | **Ctrl** (while standing still) |
| Rage of the Gods | **V** | Blade of Artemis (L1 + R1) | **Middle click** |
| Poseidon's Rage | **1** | Medusa's Gaze | **2** |
| Zeus' Fury | **3** | Army of Hades | **4** |
| D-pad | **Arrow keys** | Right stick (camera) | **I J K L** |
| Power menu (Start) | **Enter** | Select | **Tab** |

Notes:

- Ctrl deliberately does two jobs: **R2** when you are standing still and **evade** when you move. The runtime only
  counts the key as a button while no movement key is held.
- Keys are bound by **physical position**, so the layout works on any keyboard layout (including ABNT2 and Turkish).
- Gamepad buttons can be swapped one by one (✕ ○ □ △); the presets are **Classic (PS2)**, the default, and **Modern
  (DMC5 style)**. **Custom (every button)** lets you choose what each of the 16 buttons presses (see [Gamepad](#gamepad)).

## In-game shortcuts

| Key | What it does |
|---|---|
| **F11** | Toggle fullscreen |
| **F2** | Open / close the cheat menu (also on a gamepad button or combo, see below) |
| **F1** | Developer panel (frame timings, renderer state). Not needed to play |
| **Esc** | Does nothing by default. Turn on **ESC closes the game** in **Controls** to make it quit |

## Cheat menu (F2)

**F2** opens an in-game menu; the game keeps running behind it.

- **↑ ↓** (or W / S) choose, **Enter / Space / ← →** toggle, **F2 / Esc / Backspace** close.
- With the mouse: hover chooses, left click toggles.
- With a gamepad: d-pad chooses, ✕ toggles, ○ closes. The launcher can also capture **one button or two buttons held
  together** to open it (Gamepad capture is Windows-only); F2 always works.
- While the menu is open the game receives no input, so you cannot move by accident.

The cheats (the same list appears under **Cheats** in the launcher, where you choose which ones start enabled):

| Cheat | Effect |
|---|---|
| God mode | Kratos takes no damage and his health stays full |
| One-hit kills | Enemies and bosses die in one hit (Kratos is never affected) |
| Infinite magic | The magic meter never drains |
| Infinite Rage of the Gods | The Rage meter is always full |
| Max red orbs | 60,000 orbs, the game's cap |
| All upgrades | Blades of Chaos level 5, magic and Blade of Artemis level 3 |
| All powers and weapons | Zeus' Fury, Medusa's Gaze, Poseidon's Rage, Army of Hades, Blade of Artemis, Poseidon's Trident |
| Max health and magic | Every Gorgon Eye and Phoenix Feather already applied |
| Unlock all extras | God Mode difficulty, costumes, videos and Challenge of the Gods |

What you change in game is **kept for the next session**: when the game ends the launcher takes the cheats you left on, and its
settings (the **Cheats** list, the Performance tab, the Shade Boost sliders) show them from then on, so the next time you play the
game starts with them (see [Settings that follow you](#settings-that-follow-you)). `unlock_extras` also stays on the memory card once
you save.

**Port options in the game's own menus.** When the game is started from the launcher, the pause menu (Select) has one more
entry, **Port Options**, right below Options, and so does the title menu (after Press Start): **Port Options** comes after Treasures,
the last of the items you scroll through with up and down (started by hand it needs the environment variable
`GOW_NATIVE_CHEATS_MENU=1`, and `GOW_NATIVE_CHEATS_MENU=0` turns it off for a launcher session). Both open the same list of the
features of this port, with five items, and they work on the same switches:

- **Cheats** opens a screen with the same nine cheats as the F2 menu (← → or ✕ turn the highlighted one on or off) and **Back**.
  Each is the same switch as in the F2 menu, so changing one changes the other.
- **In Game Overlay** opens a screen with one ON/OFF row for each part of the performance overlay: **FPS Counter**, **GPU Stats**
  (usage, temperature, video memory), **CPU Stats** (usage, clock) and **RAM Usage** (what the game itself occupies). The
  launcher's Performance tab decides what starts on; the menu changes it on the spot and the launcher keeps what you left (see
  [Settings that follow you](#settings-that-follow-you)). Whatever the system cannot report is simply not drawn.
- **Image Adjustments** opens a screen with four sliders, **Brightness**, **Contrast**, **Gamma** and **Saturation**, like the game's
  own volume bars (← → change the value by 2 %, from 0 to 100, the middle being the unchanged picture; hold the direction and it
  repeats, after a short delay, ten steps a second; the bar has 11 positions, so it moves every fifth step while the picture changes
  with each one), and **Reset to Default**, which puts the four back in the
  middle. They are the launcher's Shade Boost settings: the Performance tab decides where they start, the menu changes them on the
  spot, the picture, menu included, follows as you move, and the launcher keeps the values you left.
- **Image Quality** opens a screen with the launcher's **Image quality** settings: **Resolution** (1x to 8x the native size, the
  internal render scale), **Upscaling** (Bicubic, Bilinear or Pixel), **Sharpness** (a slider, in steps of 5 %, for the bicubic
  filter) and **FXAA** (ON/OFF). They take effect on the spot: the render targets are rebuilt at the new resolution with the current
  picture copied into them (a moment of softness at 8x is normal: it is heavy for the graphics card), and the window scaling changes
  with the next frame. Without Bicubic, Sharpness does nothing. The graphics preset and the progressive scan (480p) switch of the
  launcher are not in the menu: changing the video mode while playing reloads the GS and corrupts the models' textures, so progressive
  scan stays a launcher setting that applies when the game starts.
- **Frame Rate** opens a screen with the launcher's **Frame rate** settings: **FPS Limit** (60 or 120; ← → or ✕ switch between the two), **VSync** and **Early FINISH** (ON/OFF). They take effect on the spot: the game logic changes to the new rate, the window
  switches VSync, and Early FINISH is switched once the GS worker has finished what it was drawing. 120 is the highest, as in the
  launcher; with the game's 30 FPS mode (`PC_GAME_FPS=30`) the limit row does nothing, and without the GS worker Early FINISH
  does nothing. The values are kept for the next session like the others.

The Options menu itself is the original one. It works with the retail USA game only, and none of this appears while
RetroAchievements Hardcore is on (the launcher's switches still work then). Back and Triangle return one level. Developer details: `docs/dev/NATIVE_MENU_RESEARCH.md`.

## Settings that follow you

The cheats (F2 menu and Port Options), the performance overlay lines, the four image adjustments, the image quality (resolution,
upscaling, sharpness, FXAA) and the frame rate (FPS limit, VSync, Early FINISH) are saved when you change them
inside the game: a moment after each change the game writes them to a small file in the launcher's data folder, and the launcher
reads it when the game ends (or the next time you press Play, if it was closed meanwhile) and updates its own settings. The
launcher's Cheats and Performance tabs then show what you left, and the next session starts with it. Nothing else of the settings
is touched. Notes:

- It needs the game started **from the launcher**; a game started by hand (no `GOW_INGAME_STATE` in the environment) saves nothing.
- If you also change those settings in the launcher while the game is running, what you set in the game wins when the game ends.
- With RetroAchievements **Hardcore** the cheats are never saved (there are none in that mode); the overlay and the image still are.
- **Reset** in the launcher's settings, and its Cheats tab, still work as before: they edit the same values.

## Target lock (Alt)

A lock-on in the style of the modern God of War games, always available:

| Input | Effect |
|---|---|
| **Alt**, pressed and released alone | Locks the closest enemy (within 1500 units, favoring the center of the screen); press again to release |
| **Mouse left / right** (60 px) | Switches to the next enemy on that side |

- It only locks when Alt was pressed *alone* and for less than 0.6 s, so **Alt+Tab** never locks anything.
- Attacks go to the locked enemy; when it dies or gets farther than 3000 units the lock moves to the closest one.
- A small red diamond marks the target.
- The camera cannot be locked (the game's camera is scripted), so this is a targeting aid, not a modern third-person
  camera.

## Agile combat

On by default (**Controls → Agile combat**). It changes only the *ends* of the game's own movements:

- an attack from a dodge comes out earlier (20 % of the roll instead of 50 %);
- an attack press the game refused (for example on the frame you release block) is retried for a moment;
- cancelling an attack with block or evade is **already in the original game** and is untouched.

If you prefer the original timing, turn the switch off.

## Gamepad

- Any XInput (Windows) or evdev (Linux) pad works; DualShock 4, DualSense, Xbox and 8BitDo controllers were tested.
- The PS2 layout is the default; the modern layout and the four face-button swaps are in **Controls**.
- **Custom (every button)** in **Controls** gives each of A, B, X, Y, LB, RB, LT, RT, Back, Start, both stick presses and the
  four d-pad directions its own PS2 button (or Nothing). It starts from the layout you were using. Two buttons may press the
  same PS2 button; Start has to stay on some button. The sticks and the guide button are not remappable.
- **Choosing the controller**: the game normally follows the first controller that answers. A receiver or a PC with several
  devices (a 2.4G dongle lists a media-keys device, keyboards and the real pad) can make that the wrong one: **Controls →
  Controller** lists what the game saw the last time it ran, marks the devices the system has no button layout for, and lets
  you pick the one to read. The list fills in after the first run with the controller connected.
- **A controller with no button layout** (many generic 2.4G receivers: the game sees it but every button reads as released)
  can be mapped by hand: choose it, press **Map its buttons**, start the game with it connected (the keyboard works in the
  menus), and in the dialog press **Listen** next to each button and press it on the controller. The game reports which
  input moved, so no numbers have to be typed. **Start** is required. The mapping applies the next time the game starts.
- **Vibration works on Windows only**: the Linux build has no rumble.
- Only port 1 is connected: the game's second controller slot does nothing.

## Saves and the memory card

- Saves are stored in a **PCSX2-compatible card image, `Mcd001.ps2`**, next to the game executable (`game/` inside your
  installation). It is an 8 MB card with ECC, the same format PCSX2 uses.
- **You can copy your PCSX2 card there** (any `Mcd001.ps2` / `Mcd002.ps2`) and the game will list those saves. The
  reverse works too: PCSX2 opens the card this port writes.
- Override the path with `GOW_MC0=<file>`. `GOW_MC0=0` leaves slot 1 empty; `GOW_MC1=<file>` enables slot 2.
- Loading and saving were verified end to end (list with thumbnails, "Load Complete", overwrite, "Save Complete").
- The `mc0/` folder next to `SCUS_973.99` holds the few `mc0:` file operations the game performs outside the card
  image. It is created automatically.
- The installer creates the card **already formatted** when there is none (an existing card is never replaced).
  Formatting a blank card from inside the game has not been verified, so avoid copying in a blank card.
- Before the first save of each session the card is copied to **`Mcd001.ps2.bak`**: if a crash or a power loss cuts a
  save, close the game and rename the `.bak` back to `Mcd001.ps2`.
- A card file of the wrong size (cut, empty, or a 16/32 MB card) is never written over: it is renamed to
  `Mcd001.ps2.unusable` and the game gets a blank card. `ps2-tools mc-check Mcd001.ps2` checks a card, and
  `ps2-tools mc-format <file>` creates a formatted one.

## Custom textures

PCSX2 replacement packs work here. **Textures → Custom textures** enables them, and the launcher can download the
community pack for you. Details, including how a pack's file names are computed, are in [`TEXTURES.md`](TEXTURES.md).

## Achievements (RetroAchievements)

**Achievements** in the launcher signs in with your RetroAchievements account and loads the game's achievement set when
the game starts.

- The account password is **not** stored with the settings: it goes to the Windows Credential Manager. On Linux the
  password store is not implemented yet, so the sign-in cannot be saved there.
- RetroAchievements only recognises the original USA disc. With a modified disc (a translation, an undub or a mod,
  including one made with the wizard's `.xdelta` option) the game plays normally, but the Achievements tab says so and
  nothing is sent.
- **Hardcore mode** is required for hardcore achievements and **disables every cheat** (the cheat list is not even sent
  to the game).
- In-game notifications can be switched off.

## Language

The launcher is in **English** and **Brazilian Portuguese** (*Other → Language*). The in-game cheat menu follows the
same choice.
