# Gameplay hooks: cheats, target lock, agile combat, pad

The PC features that change *gameplay* are C++ wrappers around specific game functions. They are registered at startup
by `ps2xRuntime/src/game/gow_overrides.cpp` and they all follow the same contract: **call the original function for
everything you do not replace**, and respect resumptions (a `ctx->pc` different from the entry means "call the original
and do nothing else").

The user-facing description of these features is in [`../PLAYING.md`](../PLAYING.md). This page is about the
implementation.

| Feature | Files | Entry point |
|---|---|---|
| Cheats and the F2 menu | `src/lib/gow_cheats.cpp` | `PC_CHEATS`, `PC_CHEATS_SHORTCUT` |
| Target lock (Alt) | `src/lib/gow_targeting.cpp` | `GOW_TARGETING=0` disables |
| Agile combat | `src/lib/gow_animation_cancel.cpp` | `GOW_CANCEL=0` disables |
| libpad2 ↔ libpad bridge | `src/game/gow_pad2.cpp` | `PC_KEYS`, `PC_DMC5_CONTROLS`, `PC_PAD_FACE_MAP` |
| Widescreen hook | `src/lib/gow_integration.cpp` (`gowWidescreenVBlank`) | `PC_WIDESCREEN` |

## Cheats

The cheats are **wrappers of the game's own functions**, not values written blindly into memory:

| Cheat | Hook | What it changes |
|---|---|---|
| `god_mode` | `0x1E0A80` (set health), `0x1E0A98` (subtract damage) | Damage to Kratos is 0 and health is refilled every VBlank |
| `one_hit_kill` | same two functions | Damage to another creature covers its whole health; every threshold is notified, so boss phases still advance |
| `infinite_magic` | `0x302D1C` | Magic value = maximum for the current level (`0x3028F4 + 4 × level`) |
| `infinite_rage` | `0x302D20` | 100, the cap of `AddRage` (`0x1DA938`) |
| `max_orbs` | `0x302D28` | 60,000, the cap of `SetOrbs` (`0x1DB250`) |
| `all_upgrades` | `0x302D38`–`0x302D3D` | Blades level 5, magic and Artemis level 3 |
| `all_powers` | `0x302D42` | `\|= 0x017C` (Zeus, Medusa, Poseidon, Hades, Artemis, trident) |
| `max_meters` | `0x302D36`, `0x302D37` | Health level 4, magic level 5 (the caps of `0x1D8200`) |
| `unlock_extras` | `0x2A12C8` | `\|= 0x1F`: completed difficulties and Challenge of the Gods |

Before writing to Kratos (`*(*0x29CD58)`), the maximum health at `+0x17C` is checked against the level table. That is
what keeps the wrappers from writing into an object freed during a load, and why `one_hit_kill` can never hit Kratos.

The menu itself is drawn with raylib over the game window. While it is open the pad read returns a neutral packet, so
the game keeps running but receives no input. `SetExitKey(KEY_NULL)` stops Escape from closing the window while the
menu is up.

Diagnostics: `PC_CHEATS_DIAG=1` (state every 2 s), `PC_CHEATS_OPEN_IN=<s>` (opens the menu at a given time, used by
capture tests).

## Target lock

The game already has auto-aim: `0x1D78B0` picks a target with `0x1D90F0` (or `0x1D9750` in manual mode) and stores its
identifier in the attack state. The lock wraps **those two functions**: after the game makes its own choice, the wrapper
returns the locked enemy's identifier instead, so attacks aim at the lock without any change to the attack logic.

Game data used:

| Address | Content |
|---|---|
| `*(*0x29CD58)` | Kratos (0x360-byte creature): position `+0xC0`, axes `+0x90/+0xA0/+0xB0`, health `+0x178`, max `+0x17C`, height `+0x124` |
| `*0x29E414` | Creature pool: header `0x31415927`, 32 entries of 0x20 from `+0x20` |
| `*0x29D2AC` | Identifier table: 12-byte entries from `+8` (`identifier`, `class`, `object`), index = `identifier >> 23` |
| `*0x35FA98` | Camera: world matrix `+0x00`, view-projection `+0x40` (row-vector, screen Y down) |
| `0x30290C` / `0x302910` | "Targeting Range" / "Manual Targeting Range" |

Selection: the closest enemy within 1500 units, favoring the screen center. The lock releases beyond 3000 units or when
the target dies. Alt counts only if pressed and released alone, in under 0.6 s, with the window focused. Mouse movement beyond 60 px
cycles to the next visible enemy on that side. The marker is a red diamond at `position + 0.55 × height`, projected with
the camera matrix.

Research switches (all local; the dumps contain game RAM and are never uploaded): `GOW_TARGETING_STICK`,
`GOW_TARGETING_DUMP`, `GOW_TARGETING_TEST`, `GOW_TARGETING_DIAG`.

## Agile combat (animation cancelling)

The game's movement controller lives at `*(Kratos+0x324)`:

| Offset | Content |
|---|---|
| `+0x44` | Requested next combo hit (pointer, 0 = none) |
| `+0x4C` / `+0x54` | Current transition / movement record in the level table (0x18 bytes) |
| `+0x50` | Movement counter |
| `+0x58` | Movement identifier (0x50 standing, 0x47–0x4B square combo, 0x20 triangle, 0x30–0x22 block, 0x201–0x208 dodges, 0x64–0x6C hits taken) |
| `+0x1AC` | State (1 normal, 3 dodge start, 0x18 attacking, 0x19 blocking) |
| `+0x1C0` | Transition queue: `{+0 pointer, +4 u8 size (8), +5 u8 read, +6 u8 write}`, 8-byte entries `{transition, threshold}` |

`0x2048F8` pops a transition when the normalized progress reaches its threshold; `0x2088E8` starts a movement;
`0x203F90` is the "animation ended / hit received" path. The hook rewrites only **the thresholds** it needs:

| Transition | Original | With the hook |
|---|---|---|
| Attack from a dodge | 0.5 | 0.2 (`GOW_CANCEL_EVASION`) |
| Block / dodge from any attack | 0 | unchanged (already immediate in the game) |
| Square combo chain | 0.27 / 0.35 / 0.47 / 0.37 / 0.45 | multiplied by `GOW_CANCEL_COMBO` (default 1) |
| Hits taken (0x64–0x6C) | - | never changed |

The input buffer re-sends a refused square/triangle press every 35 ms, at most 3 times over 0.35 s, and stops as soon
as a transition is queued or a new attack starts. `GOW_CANCEL_BUFFER=0` disables it. It only acts in gameplay (game state 11,
`0x29E560`): in the pause menu nothing ever "accepts" a Triangle, so the buffer used to press it again up to three times and one
tap went back several menu levels (Options, pause menu, game); outside gameplay it is reset on every read.

Diagnostics: `GOW_CANCEL_DIAG=1` (every movement start and queue insertion), `GOW_CANCEL_SEGMENT`,
`GOW_CANCEL_AXIS`, `GOW_CANCEL_DUMP`, `GOW_CANCEL_WATCH` (write-protects a page and logs the RIP of each write,
Linux x86-64 only).

## Pad: libpad2 and libpad

God of War uses **libpad2** (sockets), while the input backend from the upstream runtime delivers libpad packets. `gow_pad2.cpp`
connects the two for port 1 and builds the packet the game expects: buttons, both sticks, and twelve pressure values
(0 or 255).

- `PC_KEYS="action=code,code;..."`: the launcher's map. Codes are raylib/GLFW key codes; `1000 + n` are mouse buttons.
  Actions: `up down left right cross circle square triangle l1 r1 l2 r2 l3 r3 start select rage`, the four stick
  directions `ls_*`/`rs_*`, `rs_mod`, and the game-specific `poseidon medusa zeus hades artemis`.
- `rs_mod` (Ctrl) only counts as a button when no movement key is held, which is how Ctrl can be both R2 and dodge.
- `PC_PAD_FACE_MAP=cross,circle,square,triangle` encodes the A,B,X,Y swap; an invalid permutation falls back to the
  preset.
- The backend picks the pad that received the most recent press, so an idle steering wheel does not take over slot 1.

A game-independent test (`native/tests/pad2_packet_test.cpp`) checks the packet layout against the table the game uses.

## Widescreen hook

`gowWidescreenVBlank` writes the projection factor and the video flag at `0x7F00`/`0x7F04` (kernel memory the game does
not use) when the scheduler starts and on every VBlank. The recompiled code reads those addresses instead of an
immediate (`gow-port.toml` patches `sub_00169238` and `0x21E4F0`). Without `PC_WIDESCREEN` the factor stays `0.75`, the
data part of the patch is not applied and the game draws its original 4:3 picture bit for bit.

Details and the rest of the presentation path: [`RENDERING.md`](RENDERING.md).
