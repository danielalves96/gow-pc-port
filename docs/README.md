# Documentation

These are the docs for a **static recompilation** of *God of War*: the game's executable is translated to C++ and
compiled, with **no emulator, no MIPS interpreter and no BIOS**. The [main README](../README.md#not-an-emulator) has a
short explanation of the distinction; the implementation is described in [`dev/ARCHITECTURE.md`](dev/ARCHITECTURE.md).

They are split in two: what a **player** needs (this folder) and what a **contributor** needs
([`dev/`](dev/)). If you are here to play, you can stop after the first table.

## Start here

| I want to… | Read |
|---|---|
| Install the game | [`INSTALL.md`](INSTALL.md) |
| Use the launcher (tabs, options, logs, paths) | [`LAUNCHER.md`](LAUNCHER.md) |
| Play: keys, cheats, saves, gamepad, achievements | [`PLAYING.md`](PLAYING.md) |
| Fix a problem | [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) |
| Know what works and what does not | [`STATUS.md`](STATUS.md) |
| Make the game run faster | [`PERFORMANCE.md`](PERFORMANCE.md) |
| Use or build texture packs | [`TEXTURES.md`](TEXTURES.md) |
| Know whose code this is built on, and under which licenses | [`THIRD_PARTY.md`](THIRD_PARTY.md) |

## Player documentation

| Page | Contents |
|---|---|
| [`INSTALL.md`](INSTALL.md) | Requirements, the Windows wizard, both installer scripts and every option, what the installation creates, the optimization pass, updating and removing |
| [`LAUNCHER.md`](LAUNCHER.md) | The eight tabs and every setting with its default, the setup wizard, what PLAY does, where settings, logs and the shader cache live |
| [`PLAYING.md`](PLAYING.md) | Default keyboard and mouse layout, in-game shortcuts, the cheat menu, target lock, agile combat, gamepad, saves and memory card, achievements |
| [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) | Installation, startup, performance, audio, saves and controls, by symptom; where the logs are; what to attach to a report |
| [`STATUS.md`](STATUS.md) | Platform support, what works, known limitations, open problems (including the experimental Linux state) |
| [`PERFORMANCE.md`](PERFORMANCE.md) | Real-time speed and the FPS limit (60 or 120, the highest), what costs what, measured reference points, tuning order, how to check real speed |
| [`TEXTURES.md`](TEXTURES.md) | Installing a pack, making one (file naming and hashes), memory budgets, what is not replaced |
| [`THIRD_PARTY.md`](THIRD_PARTY.md) | Every upstream project and library, what is used, where it lives, its license |

## Developer documentation

| Page | Contents |
|---|---|
| [`dev/README.md`](dev/README.md) | Index and the five-minute orientation |
| [`dev/ARCHITECTURE.md`](dev/ARCHITECTURE.md) | Recompiler, runtime, overrides, threads, operating rules, where a change goes |
| [`dev/BUILD.md`](dev/BUILD.md) | Building by hand, every CMake option that matters, VU1/IOP generation, tests, CI |
| [`dev/ENVIRONMENT.md`](dev/ENVIRONMENT.md) | Environment-variable reference: launcher set, presentation, pacing, diagnostics |
| [`dev/PERFORMANCE.md`](dev/PERFORMANCE.md) | Measurement procedure, counters, how to reduce a regression, benchmarks without the game |
| [`dev/RENDERING.md`](dev/RENDERING.md) | GS backends, frame path, presentation, widescreen hook, texture replacement, verification tooling |
| [`dev/AUDIO-VIDEO.md`](dev/AUDIO-VIDEO.md) | Sound engine and IOP audio, MPEG movies, A/V synchronization |
| [`dev/MEMORY-CARD.md`](dev/MEMORY-CARD.md) | Card image, IOP services, the fixes that made saving work, what is unverified |
| [`dev/GAMEPLAY-HOOKS.md`](dev/GAMEPLAY-HOOKS.md) | How cheats, target lock, agile combat and the pad bridge are implemented |

The recompiler's byte-identical gate has its own document next to its code:
[`PARITY.md`](../RustPortPS2/docs/PARITY.md). The launcher's contributor guide is
[`gow-launcher/README.md`](../gow-launcher/README.md).

## How these docs are organized

- **`docs/` is for players.** Anything that only a developer needs goes in `docs/dev/`.
- **One subject per file**, with no dated session logs, personal paths or measurement dumps. If a number matters, it
  goes in a table along with the machine it came from.
- Everything is written in **English**, except `README.pt-BR.md`, the launcher's translations and the in-game cheat
  menu.

When you change behavior, update the page that describes it. If the code and a page disagree, the code wins: fix the
page in the same change.
