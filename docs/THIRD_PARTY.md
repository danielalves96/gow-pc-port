# Third-party code, credits and licenses

This port was not written from scratch. It builds on seven upstream projects (the recompiler, the C++ runtime, a
God of War-specific patch stack, the OpenGL GS backend, PCSX2 as the hardware reference and two smaller projects that
supplied ideas) plus a handful of small libraries. This page is the inventory: **what** is used, **where** it lives in the tree, and **which license**
applies. It is meant to be complete enough to audit.

The repository itself is **GPL-3.0** (see [`LICENSE`](../LICENSE)). Every GPL-3.0 and GPL-3.0-or-later part below is
therefore compatible with it; the permissive libraries keep their own licenses and their notices stay in the tree.

> Nothing here redistributes game content. *God of War* © Sony Interactive Entertainment / Santa Monica Studio. This
> project is not affiliated with or endorsed by Sony.

---

## 1. Projects this port is built on

| Project | Author | What this repository uses | Where | License |
|---|---|---|---|---|
| [PS2Recomp](https://github.com/ran-j/PS2Recomp) | ran-j and contributors | The PlayStation 2 static recompiler whose output `ps2_recomp` reproduces byte for byte, and the C++ runtime (EE, GS, VU, IOP, kernel stubs, pad, SIO) this port builds on. | `RustPortPS2/crates/`, `RustPortPS2/native/` | GPL-3.0 |
| [God-Of-War-Native-Port](https://github.com/Waffle565/God-Of-War-Native-Port) | Waffle565 | The runtime that runs the game today: native GS driven by render targets, IOP modules compiled ahead of time, native VU1 blocks, videos through libmpeg2, the sound engine, the native memory card and the base of the game overrides. The PC features sit on top of it. | `RustPortPS2/native/ps2xRuntime/`, `ps2xIOP/`, `src/game/` | GPL-3.0 |
| [god-of-war-recomp](https://github.com/KIexster/god-of-war-recomp) | KIexster | The recompilation pipeline for this exact game: the 68-patch PS2Recomp stack, the function map, the patch set of `gow-port.toml`, the VU1 C++ generator (`generate_vu1.cpp`) and the game-speed meter idea (`GOW_DELTA_DIAG`). | `RustPortPS2/games/scus-97399/`, `RustPortPS2/native/tools/vu1/`, `runtime/ps2_perf.h` | see upstream repository |
| [PS2Recomp `sotc-port`](https://github.com/LightVelox/PS2Recomp/tree/ac9efa070638ad3b3accd284de6f898d5ab271d1) | Taylor N. Albarnaz / LightVelox | The whole OpenGL side of the GS: GL context and shared presentation, threaded backend, swizzle, GS memory, backend state, texture page cache, GS replay tooling, triangle rules and thread priority. Commit `ac9efa0`. | `RustPortPS2/native/ps2xRuntime/src/lib/gs/`, `include/runtime/gs/`, `include/gow_gs_replay.h`, `include/ThreadPriority.h` | GPL-3.0 |
| [PCSX2](https://github.com/PCSX2/pcsx2) | PCSX2 Dev Team | Reference implementation of the hardware: the EE and VU1 interpreters used to verify the port (`ee_ref/`, `vu_pcsx2/`), VIF unpack behavior, FPU/rounding behavior, timer semantics, the texture-replacement hashing algorithm and the widescreen patch this port builds on. | `…/src/lib/ee_ref/`, `…/src/lib/vu_pcsx2/`, `…/src/lib/gs/gpu/gs_texture_replace.cpp` | GPL-3.0+ |
| [SOCOM Unzipped](https://github.com/Scotho/socom-unzipped) | Scotho | The XGKICK packet-snapshot idea (`GOW_XGKICK_IMMEDIATE`) and a second reference for VIF unpack. | `…/src/lib/vu/ps2_vu1_core.cpp` | see upstream repository |
| [ps2-port-engine](https://github.com/danielgomesvieira2000/ps2-port-engine) | danielgomesvieira2000 | The structure of the texture-pack loader (`host/src/texture_pack.cpp`), MIT; the naming/hash algorithm itself is PCSX2's. | `…/src/lib/gs/gpu/gs_texture_replace.cpp` | MIT |

The research notes of [gow-native-pc](https://github.com/Vnnc1/gow-native-pc) (GPL-3.0, release candidate 0.6.0-rc.1)
pointed this port at several places in the game: where a movie ends (`0x188848`, `0x189B00`, `0x189A88`), which call completes a
save (`0x22E800`), and how to tell shadow volumes and additive effects apart by their GS state. Their patches were not applied
and no code was copied: the addresses were checked against this port's own generated code and the implementations are
this project's (`gow_movie.cpp`, `gow_save.cpp`, `gs_frontend.cpp`).

The widescreen support is based on the **PCSX2 "Widescreen 16:9" patch for `SCUS-97399`** by **nemesis2000** and
**CRASHARKI**. The patch's instruction words are stored (with attribution) in
[`gow-port.toml`](../RustPortPS2/games/scus-97399/gow-port.toml).

---

## 2. Libraries bundled in the repository

| Library | Version | Used by | Where | License |
|---|---|---|---|---|
| [Rabbitizer](https://github.com/Decompollaborate/rabbitizer) | 1.14.3 | MIPS disassembly text in the commented C++ the recompiler emits | `RustPortPS2/third_party/rabbitizer/` | MIT |
| [xxHash](https://github.com/Cyan4973/xxHash) | single header | XXH3-64 texture hashing of the replacement packs | `…/ps2xRuntime/third_party/xxhash/` | BSD-2-Clause |
| [stb_image](https://github.com/nothings/stb) | single header | Decoding `.png` replacement textures | `…/ps2xRuntime/third_party/stb/` | MIT or public domain |
| [bcdec](https://github.com/iOrange/bcdec) | single header | Decoding `.dds` (BC1-BC7) replacement textures | `…/ps2xRuntime/third_party/bcdec/` | MIT or Unlicense |
| [libmpeg2](https://libmpeg2.sourceforge.io/) | — | MPEG-2 decoding for the game's movies | `…/ps2xRuntime/third_party/libmpeg2/` | GPL-2.0+ |
| [rcheevos](https://github.com/RetroAchievements/rcheevos) | 12.5.0 | RetroAchievements: protocol, condition evaluation, game identification by hash | `…/ps2xRuntime/third_party/rcheevos/` | MIT |

Each folder keeps its own license file (`LICENSE`, `COPYING`). The C++ runtime also carries the inherited
[`LICENSE`](../RustPortPS2/native/LICENSE) of the upstream runtime.

## 3. Libraries downloaded at build time

These are fetched by CMake when the game is built for the first time, so they are not stored in this repository:

| Library | Version | Used by | License |
|---|---|---|---|
| [raylib](https://github.com/raysan5/raylib) | 5.5 | Window, input, OpenGL context, presentation and the F2/F1 overlays | zlib |
| [Dear ImGui](https://github.com/ocornut/imgui) | v1.92.7-docking | Developer panel (F1) | MIT |
| [rlImGui](https://github.com/raylib-extras/rlImGui) | `Raylib_5_5` | ImGui on top of raylib | MIT |
| [FFmpeg](https://ffmpeg.org/) | system | **Optional** alternative video decoder (`-DPS2X_ENABLE_FFMPEG=ON`); libmpeg2 is the default | LGPL-2.1+ / GPL-2.0+ |

## 4. Rust crates

The recompiler and the developer tools (`RustPortPS2/`) use very few crates: `toml`, `serde`/`serde_json` (MIT or
Apache-2.0), the vendored Rabbitizer, and `cc` to build it. The launcher (`gow-launcher/`) adds the Tauri stack and its
own helpers:

| Crate | Used for | License |
|---|---|---|
| `tauri`, `tauri-plugin-dialog`, `tauri-plugin-single-instance` | Launcher shell and window | MIT or Apache-2.0 |
| `serde`, `serde_json` | Settings, manifests, IPC | MIT or Apache-2.0 |
| `dirs` | Platform data/cache folders | MIT or Apache-2.0 |
| `image` | Launcher icon and screenshot handling | MIT or Apache-2.0 |
| `md-5`, `sha2` | ISO/ELF digests for the file checks | MIT or Apache-2.0 |
| `ureq` | Texture-pack download | MIT or Apache-2.0 |
| `zip` | Extracting the texture pack | MIT |
| `thiserror` | Error types | MIT or Apache-2.0 |
| `windows-sys`, `winreg` | Windows GPU preference, credentials, XInput | MIT or Apache-2.0 |
| `libc` | Unix process handling | MIT or Apache-2.0 |

The exact set and versions are pinned by `RustPortPS2/Cargo.lock` and `gow-launcher/Cargo.lock`; `cargo metadata` prints
the effective license of every entry.

## 5. Launcher front end

| Package | Used for | License |
|---|---|---|
| [React](https://react.dev/) 19 | Interface | MIT |
| [Tauri API](https://tauri.app/) 2 (`@tauri-apps/api`, plugins) | Bridge to the Rust side | MIT or Apache-2.0 |
| [lucide-react](https://lucide.dev/) | Icons | ISC |
| [Tailwind CSS](https://tailwindcss.com/) 4 | Styling | MIT |
| [Vite](https://vite.dev/), [Vitest](https://vitest.dev/), TypeScript | Build and tests | MIT / Apache-2.0 |

Versions are pinned by `gow-launcher/package-lock.json`.

## 6. Fonts, icons and other assets

- The runtime window icon and the `gow-2005` executable icon are the project's own Omega artwork
  (`RustPortPS2/native/ps2xRuntime/resources/gow.ico`).
- The launcher icon is `gow-launcher/assets/icon-source.png`.
- The F2 cheat menu and the FPS overlay draw with a system font of the host (DejaVu/Noto/Liberation on Linux,
  Segoe UI/Arial on Windows); no font file is redistributed.
- The game's own button prompts, HUD and fonts come from the game itself and are not part of this repository.

## 7. Reporting a missing credit

If something in this repository is yours and is not listed here, open an issue at
<https://github.com/danielalves96/Gow-pc-port>. Every imported file keeps a header naming its origin, so running
`git grep` for `GOW-Port:` and for the upstream names above is the fastest way to check a single file.
