# Toaster plan: a steady 60 FPS on the weakest PC (items 6 and 7)

The goal of this work is that **any PC, however weak, holds 60 FPS without stutter**. It continues a series of
optimizations taken from two other PS2 static recompilations,
[gta-vcs-recomp](https://github.com/gothamor/gta-vcs-recomp) (its `docs/PERFORMANCE.md` and
`docs/PS2RECOMP_PATCHES.md`) and [GHClassic](https://github.com/noahbaxter/GHClassic). Items 1 to 5 of that list are
done (commits `0b5f851`, `8bf3eea`, `30e627b`, `2f20698` on `release-0.3.4`). This document covers the two that are left:

- **Item 6: is the PGO training still worth it?** If not, drop it, so that installing and optimizing never needs the
  game to open by itself.
- **Item 7: the VU1, the biggest share of the frame.** Make it cheaper.

Everything in [`AGENTS.md`](../../AGENTS.md) applies, in particular: never above 120 FPS and no uncapped tests; warn the
user and wait before launching the game; never close a `gow-2005` by name; builds at Idle priority; never commit game
files, generated code or captured VU1 microcode; `// GOW-Port:` on upstream code.

## Where the frame goes today

The release build, the launcher's real environment (60 FPS, `PS2X_GS_WORKER=1`, 4x internal scale), pinned to 4 cores
(`scripts/ab-perf.ps1 -Cores 4`), the opening scene of the game, 3 alternating pairs of 150 s (run of 2026-11-11):

| Thread | Time per game frame |
|---|---|
| VU1 | 6.9 – 9.1 ms |
| GS (worker) | 5.9 – 7.8 ms |
| EE (game code) | 4.1 – 5.5 ms |

60 FPS gives 16.7 ms per frame. On a PC with half the speed per core, the VU1 alone would take ~14–18 ms: it is what
stands between a weak PC and 60 FPS. No frame above 50 ms after the opening in those runs (`GOW_FRAMES_HITCH_LOG=1`).

**"Toaster" proxy for measuring:** `-Cores 2` (and `-Cores 4`), internal scale 1x and 2x, the launcher's environment at
60 FPS (`cargo run -p gow-cli -- env` from `gow-launcher/`, nothing overridden except `GOW_MC0` for a copy of the
memory card and `GOW_FRAMES_HITCH_LOG=1`). Success means: guest frame rate 60.0 in every 5 s window, frames over 16.7 ms
close to 0, no hitch above 50 ms after the opening. Compare builds only against a build of the previous commit with the
same configuration (`docs/dev/PERFORMANCE.md`), by the per-thread time per frame from `[gow-perf]` (`ee_ms`, `vu_ms`,
`gs_ms` divided by the frames of the window), which shows a gain even when both builds hold 60.

## Item 6: PGO

What it is today: clang PGO. `install.ps1 -Pgo on` (and the launcher's **Optimize now**, `gow-core/src/optimizer.rs`)
builds an instrumented executable (`-fprofile-instr-generate`, detected by `is_instrumented`), opens the game by itself
with scripted presses for 240 s ("the game opens by itself for 240 s, do not touch the keyboard"), merges
`build/pgo/gow.profdata` and rebuilds with it. The VU1 microprogram capture (`GOW_VU1_CAPTURE`, rebuilt in the
background by the progressive optimizer) is a different thing and stays.

gta-vcs-recomp found that after its codegen work PGO gave only 5–10 % on Windows and nothing measurable on Linux. Our
codegen changed the same way (one-compare memory access, lean `ctx->pc`, `__restrict` context, inline dispatch).

To do:

1. Measure: a final build (`install.ps1` without `-FastDev`, AOT VU1) **with** the profile against the same commit
   **without** it, on the toaster proxy (`-Cores 2` and `-Cores 4`, 60 FPS, real environment), per-thread time per
   frame, 3+ alternating pairs. A profile trained before the codegen changes is stale: train a new one first (that
   training run opens the game: ask the user).
2. Decide with the numbers: below ~5 % on the EE and VU threads, or within noise, propose removing the training step
   (installer, launcher, docs) and keeping the plain release build. Above that, look for a way to keep the gain without
   the training run: a profile generated without the game (the VU1 replay bench and the GS replay tools in
   `native/tools/`), or a profile shipped as data derived only from our own code (check that it holds nothing from the
   game before proposing it).
3. Whatever the outcome, the installer must keep working on a PC where the game never opens during installation.

Deliverable: the measured table, the decision, and if it is removal, the change to `install.ps1`, `install.sh`,
`gow-core` (optimizer and its UI strings, both languages), `README.md` / `README.pt-BR.md` (in sync) and `docs/`.

### Item 6 results (2026-11-11) and decision: keep PGO, ship a runtime-only profile

Final build, launcher environment, 60 FPS, opening scene, 3 alternating pairs of 150 s (the runtime-only profile: 2 pairs),
per-thread time per frame:

| Build | Cores | VU1 ms/frame | EE ms/frame | GS ms/frame |
|---|---|---|---|---|
| no PGO | 4 | 7.96 | 5.07 | 6.69 |
| full profile trained on the PC (`install.ps1 -Pgo on`) | 4 | 6.16 (-23 %) | 4.03 (-20 %) | 4.26 (-36 %) |
| no PGO | 4 | - | - | 6.37 |
| runtime-only profile (shipped) | 4 | about -2 % | about 0 | 4.85 (-24 %) |
| full profile vs no PGO | 2 | within noise: the game cannot hold 60 FPS there either way | | |

Every PGO run of the full profile beat every non-PGO run. The runtime-only profile is the full one with the recompiled EE
functions, the generated VU1 blocks and compiled programs, and the templates instantiated with microcode words removed
(`llvm-profdata merge --no-function='sub_[0-9A-F]{8}|entry_[0-9A-Fa-f]+|vu1n_part|vu1c_gen|IL[jmiy][0-9]{3,}E'`: 13,398 of
162,811 functions kept, 2.3 MB). Its gain is the GS; the VU1 and EE gain needs the training on the player's PC.

Decision: PGO stays (well above the 5 % threshold), and the installation never needs the game to open by itself:

- by default (clang) the build uses `RustPortPS2/native/pgo/runtime.profdata`, shipped in the repository (holds nothing from the
  game: names audited, no value-profile data, see `docs/dev/BUILD.md`);
- the full training stays as the optional **Optimize now** / `-Pgo on` / `--pgo on`; the locally trained profile then wins
  over the shipped one;
- `scripts/make-runtime-profile.ps1` turns a freshly trained full profile into the shipped one and audits it; refresh it after
  runtime changes and before a release (functions that changed since simply lose their profile, the build stays quiet);
- MSVC and GCC builds are unaffected.

## Item 7: the VU1

What exists: VU1 microprograms are compiled ahead of time from the captures (`PS2X_VU1_MICROCODE_DIR`,
`native/tools/vu1/generate_vu1.cpp`, `ps2-tools vu1n-gen`; the build log shows
`[generate_vu1] 7796 pair functions, 2240 blocks, 6207 of 10307 FMACs with dead flags, 4096 images`), with an
interpreter fallback and cycle-accurate XGKICK progress (`src/lib/vu/ps2_vu1_core.cpp`, `ps2_vu1_native.inl`). The
replay bench (`GOW_VU1N_RECORD` / `GOW_VU1N_REPLAY`, `docs/dev/PERFORMANCE.md`) measures the VU1 alone and checks every
record against its hash: **M must stay 0** for an exact change.

How the others did it: gta-vcs-recomp replaced the hot microprograms with native versions verified against the
interpreter; GHClassic skips the VU1 path and draws straight to Vulkan.

To do, in order, each step measured before the next:

1. **Profile**: which microprograms and which blocks take the VU1 time in the opening scene and in a combat scene
   (sampling with the linker map, `GOW_SAMPLING=1` + `ps2-dev sampling-win`, or counters per program). How much of it is
   native blocks, how much interpreter fallback, how much XGKICK/GIF work.
2. **Cheap wins first**: native-block misses and fallbacks (`vu1n_heads.txt` coverage), work per program call that does
   not depend on the program (setup, flag bookkeeping, XGKICK progress per cycle), more dead-flag elimination.
3. **Specialize the hot programs**: native versions of the few microprograms that dominate (SIMD, no per-instruction
   flag work when no reader needs it), generated by the tool and verified by the replay bench (hash-exact) before any
   game run.
4. Only if 1–3 leave the VU1 above ~4 ms per frame on 4 cores: a design note (not code) for a deeper change, such as
   GHClassic's direct path, with what it would cost.

Deliverable: per step, the replay bench result (records checked, M = 0, median before and after) and the toaster A/B;
commits per step with those numbers in the message.

## Rules for this work

- Exactness first: the replay bench with M = 0 for every VU1 change, the parity gate untouched (`cargo test --workspace`
  from `RustPortPS2/`), and the codegen switches behind `[codegen] mode = "gow"` so that `compat` stays byte-identical.
- Measure with the final build and the launcher's real environment; never above 120 FPS; never call something a gain
  without the alternating pairs against the previous commit.
- Runtime changes in `.cpp` files when possible: a header included by the generated code rebuilds ~6,400 units.
- Rust: `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt -p <crate>` (never `--all`).
- Commits in English, authored by the user alone (no co-author lines), as in the existing history.
