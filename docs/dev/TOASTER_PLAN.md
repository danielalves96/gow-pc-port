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

### Item 7 results (2026-11-11)

**Profile** (combat, 4 cores, final build, `GOW_SAMPLING` at 1 kHz for 120 s, symbols from `llvm-nm`): every VU1 block runs
native (0 misses, `GOW_VU1N_DIAG`), ~80 k programs and ~16 M blocks per second, ~8 pairs per block, ~3 ns per VU pair. Share of
the VU1 thread's wall time: native blocks 26 %, the `runNative` dispatch loop 13 %, `blockTiming` 3 % (its slow path),
`commitReadyPipelines` 2.6 %, VIF unpack 3 %, slot setup and code-change bookkeeping 1.6 %. The hot blocks are four
software-pipelined vertex kernels in the per-draw part of the micro memory (0x2800-0x3E00): lit and textured (4x4 transform,
one FDIV, ST*Q, 3x3 lighting), the same with a second matrix for a second ST, plain textured (two blocks that alternate), and
position only. Together ~13 % of the thread; the dispatch around them costs more than they do.

**Steps** (all runtime, `ps2_vu1_native.inl`; the VU1 replay bench on 4000 combat records, 8 reps, 3 alternating runs):

| Commit | Change | Replay bench |
|---|---|---|
| `719372f` | FDIV/EFU retire inline in the native loop; a micro-memory change tests only the slots near the changed pairs | 0 differ; median -2.7 % |
| `b5eb437`, `0258970` | a block that loops to itself, or whose next block is already set up, is entered again without the full dispatch (same per-block timing, history, flags and budget cut) | 0 differ, and the per-record hashes **with the final cycle count** (`ca74f55`) identical to the base: cycle-exact |
| all, no PGO | | median 13.26 -> 12.71 ms (-4.1 %) |
| all + the shipped runtime profile | | median 13.26 -> 12.15 ms (-8.4 %) |

**Game, final build of this branch against `d01ed93`** (shipped runtime profile, plus the GS stage, GL backend and EE thread
changes of the same branch), launcher environment, 60 FPS, opening scene, 3 alternating pairs of 150 s:

| Cores | Base | This branch |
|---|---|---|
| 2 | 41.1 FPS, worst 5 s window 20.4, p95 73.9 ms, 444 frames over 50 ms | 44.0 FPS, worst window 25.2, p95 63.5 ms, 293 frames over 50 ms |
| 4 | 60.0 FPS; VU1 7.94, EE 4.55, GS 6.83 ms/frame (19.3 in all) | 60.0 FPS; VU1 5.89 (-26 %), EE 4.16 (-9 %), GS 3.66 (-46 %) ms/frame (13.7 in all, -29 %); every run of this branch below every base run on VU1 and GS |

The first 4-core run of this branch had one 5 s window at 3.9 FPS with every thread slowed at once: the new GS queue producer
lock polled with `sleep_for(50 us)` when contended (at least ~1 ms on Windows). Fixed in `ad827be` (futex-style wait); the
confirmation run above has no window below 59.9 FPS and no hitch over 100 ms after start-up. With 2 cores the game is still
CPU-bound below 60 FPS: the next steps are the ones in the design note below plus fewer heavy threads competing.

### Round 2: total CPU on 2 cores (2026-11-11/12)

Two cores could not hold 60 FPS after round 1, so the second round went after every heavy thread and after the policies
that only pay where a core is free. All measured with the launcher's environment at 60 FPS, alternating pairs of 150 s
after a discarded warm-up run (`ab-perf.ps1 -WarmupRunSeconds`, new: the first run of a series used to meet a cool machine
and was by far the fastest, always variant A).

- VU1: loop folding in the generator (`vu1n-gen`, 85 self-looping blocks iterate inside one call), a cached registry key for
  slot setups, no x87 widening on the fast path of `normalizeFmacResult`, no empty commit at callouts. Replay bench: 0 differ,
  and the per-record hashes with the final cycle count identical to the base (cycle-exact).
- VIF1 UNPACK: contiguous-destination loops for CL == WL.
- GS: the draw state is rebuilt only after a state register write; byte compares before field compares in the backend.
- IOP: the native IRX modules are now built at installation from the disc (`ps2-tools iop-manifest`,
  `RustPortPS2/games/scus-97399/iop.manifest`; the 8 modules byte-identical to a game dump). Before, they only came from the
  optimization run, which the VU1 manifest path skips: most installations ran every IRX instruction interpreted. Import stub
  decoding is memoized and plain interpreted instructions run in a tight loop.
- Diagnostics: `ps2_perf` reads the TSC instead of `QueryPerformanceCounter` (the launcher always enables it).
- Policies that depend on the usable processors (4 or more: as before; fewer: the CPU goes to the EE and the VU1): the GS
  stage idle spin, the EE's wait for the last fraction of a millisecond (yield or high-resolution sleep), the GL thread's
  above-normal priority. See `docs/dev/ENVIRONMENT.md`.

| 2 cores, opening scene | FPS mean | worst 5 s window | p95 frame time | frames over 50 ms | hitches >= 100 ms per run |
|---|---|---|---|---|---|
| `d01ed93` (start) | 40.0 | 21.4 | 75.8 ms | 392 | 117-163 |
| `6d19eca` (round 2) | 49.8 | 32.1 | 37.1 ms | 186 | 1 (start-up) |
| `aa33f76` (+ the low-core policies) | 53.9 (vs 48.1 for `6d19eca` in the same series) | 38.9 | 32.0 ms | 81 | 1 |
| `f9dbc76` (+ refreshed shipped profile, GS frontend on the worker below 4 processors), same series as the base | 53.4 (base 38.9) | 40.6 (base 20.2) | 30.1 ms (base 78.0) | 17 (base 426) | 1-2 (base 143-187) |

Internal scale 1x or 2x instead of 4x changes nothing on 2 cores (CPU-bound). Four cores keep a steady 60 FPS; against the
base in the same final series the VU1 thread went 6.95 -> 5.78 ms per frame and the GS 5.92 -> 2.68 (EE about the same).
The full local training (Optimize now) adds ~3 % on 2 cores. `PS2X_GS_PIPELINE=0` crashed at boot before this round (the
worker pushed its markers into a stage that was never started); fixed, it is now the default below 4 processors.

**Still open:** 2 cores average ~53-54 FPS, not 60 (worst 5 s window ~40 FPS, almost no hitches). What is left is real work on
the EE, the VU1 and the GL driver sharing two cores: the VU1 kernels in SIMD (design note below), and the GL thread's own
cost (`addVertex`, draw submission) are the next candidates.

### What "2 cores" and "4 cores" were, and the dual-core result

The test CPU (Core i5-12400F) has 6 physical cores with SMT, and Windows numbers the two threads of a core next to each
other (`GetLogicalProcessorInformation`: core masks 0x3, 0xC, 0x30...). So `ab-perf.ps1 -Cores 2` pinned the game to **one
physical core** with two threads, and `-Cores 4` to **two physical cores with SMT** (a 2-core/4-thread Pentium or i3).
`-AffinityMask 0x5` (new) pins two threads on two different cores: a dual core without SMT. The low-core policies count
logical processors, so a 2C/4T PC gets the 4-processor behaviour, which is what the `-Cores 4` runs measured.

| Configuration (60 FPS cap, launcher environment, 3 alternating pairs after a warm-up run) | `d01ed93` | this branch |
|---|---|---|
| Dual core without SMT (2C/2T, `-AffinityMask 0x5`) | 46.8 FPS, worst 5 s window 31.8, p95 57.1 ms, 230 frames over 50 ms, 113-139 hitches over 100 ms per run | **60.0 FPS, worst window 59.7, p95 18.2 ms, 0 frames over 50 ms, 1 hitch (start-up)**; EE 4.0, VU1 3.4, GS 2.2 ms per frame |
| Dual core with SMT (2C/4T, `-Cores 4`) | 60.0 FPS | 60.0 FPS with much less CPU (VU1 -17 %, GS -55 %) |
| One core with SMT (1C/2T, `-Cores 2`) | 38.9 FPS | 54.4 FPS, frames over 50 ms 426 -> 8-17 |

A dual-core PC holds a steady 60 FPS now, with room to spare per thread; a single physical core does not. The test CPU is
faster per core than an old dual core: the per-frame thread times above (EE, the critical path, ~4 ms of the 16.7 ms frame)
are the margin left for slower cores.

Slower cores, approximated by a helper that spins at time-critical priority for a fixed share of every 10 ms on the same
two processors (dual core without SMT, 120 s windows, frames counted; the hitches are those over 100 ms per run):

| CPU share left to the game | `d01ed93` | this branch |
|---|---|---|
| 100 % (the test CPU) | 46.8 FPS, 113-139 hitches | 60.0 FPS, 1 hitch |
| 67 % (a dual core ~1.5x slower per core) | 41.2 FPS, 159 hitches | 54-55 FPS, 1 hitch |
| 50 % (~2x slower) | 33.3 FPS, 181 hitches | 51-52 FPS, 1 hitch |

The helper takes the CPU in 3-5 ms slices, which hurts frame pacing more than a uniformly slower CPU would, so this is a
pessimistic estimate. On a dual core much slower than the test CPU the game runs smoothly but below 60 FPS: the next step
there is still the structural work listed below.

Where the CPU goes on a dual core (90 s of combat at 60 FPS, CPU time per thread from the system, test CPU speed): the VU1
worker (VU1, VIF1 and, below 4 processors, the GS frontend) 63 % of a core, the GL thread 35 %, the AMD driver's own thread
15 %, the game thread (EE, IOP, audio) 25 %: about 1.46 of the 2 cores. A dual core 1.5-2x slower per core needs about 30 %
less in total to hold 60 FPS, and the largest share is the VU1 worker, then the video driver path. Presenting the window at
60 Hz instead of the launcher's 120 does not help (53.2 vs 54.9 FPS at 67 % CPU share, 3 runs each).

### Rounds 3 to 5 (2026-11-12): what did not pay

Each measured on 2 cores the same way (3 alternating pairs after a warm-up run, against the build before it); kept only
when it won:

| Change | Result | Kept |
|---|---|---|
| GL state shadow (two thirds of the GL state calls elided) and a vertex ring buffer | 54.9 FPS without vs 54.5 with | no (reverted) |
| Fused VU1 two-block cycles with the VU state in locals (exact, cycle-exact) | 55.5 without vs 55.0 with | no (reverted) |
| Native VBlank wait (the hottest EE function is a busy wait) | 54.9 with vs 54.4 without: noise | no (reverted) |
| EE codegen: lean pc for delay-slot entries and VU0 ops, grouped RAM range checks | 54.1 -> 54.4, frames over 50 ms 14 -> 8 | yes |
| EE codegen: constant propagation through calls by pointer, direct call dispatch (-20 % static instructions in the hottest function) | 54.6 -> 53.9 | no (not merged) |

A 2-core run with `PS2X_GS_WORKER_DIAG=1` shows the EE thread busy ~97 % of the time with no idle wait: two cores are
saturated by the EE, the VU1 worker (which now also runs the GS frontend) and the GL thread with its driver. Removing
instructions here and there no longer moves the frame rate; the next step is structural: less work on the EE critical path
(profiling the recompiled game code by function on a quiet 2-core run with a sampler that does not compete for the two
cores), the VU1 kernels in SIMD across vertices (design note below), or moving work off the two busiest threads.

**Design note: what is left for the VU1** (it stays above ~4 ms per frame on 4 cores):

- *Kernel specialization* (step 3 proper): hand-written or generated SIMD versions of the four vertex kernels that process 4
  (SSE) or 8 (AVX2, optional) vertices per instruction, with the same lane-wise operations under round-toward-zero and
  flush-to-zero so the bits stay identical, and the loop entry/exit state (flag tails, `m_cycle`, the software-pipelined
  prologue/epilogue) kept exact. Estimate: infrastructure (override hook per micro-memory pattern, cycle and tail accounting,
  a differential harness on the replay bench) 5-7 days, then 2-5 days per kernel; 4-5 weeks for the four. Ceiling: ~0.8 ms per
  frame (~10 % of the VU1 thread), because the four kernels are ~13 % of it.
- *Loop folding in the generator*: let the self-looping kernel blocks iterate inside the generated function with
  per-iteration idle-cycle accounting, removing the remaining per-iteration dispatch; 2-4 days, exact by construction, best
  next step.
- *GHClassic's direct path* (draw without executing the VU1): months, and it loses the replay bench (GIF output) as the
  exactness oracle. Not recommended.
- The rest of the thread is VIF unpack (3 %) and the GS stage push (1.9 %): cheaper to attack than the kernels.

## Rules for this work

- Exactness first: the replay bench with M = 0 for every VU1 change, the parity gate untouched (`cargo test --workspace`
  from `RustPortPS2/`), and the codegen switches behind `[codegen] mode = "gow"` so that `compat` stays byte-identical.
- Measure with the final build and the launcher's real environment; never above 120 FPS; never call something a gain
  without the alternating pairs against the previous commit.
- Runtime changes in `.cpp` files when possible: a header included by the generated code rebuilds ~6,400 units.
- Rust: `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt -p <crate>` (never `--all`).
- Commits in English, authored by the user alone (no co-author lines), as in the existing history.
