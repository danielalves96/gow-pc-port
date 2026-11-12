# Measuring performance

The user-facing advice is in [`../PERFORMANCE.md`](../PERFORMANCE.md). This page is the procedure and the counters, so
that two measurements can actually be compared.

## Where the time goes

| Subsystem | Thread | Notes |
|---|---|---|
| EE (recompiled game code) | main | Cheap per instruction; with early FINISH it prepares frame *N+1* while the worker draws *N* |
| VU1 | GS worker | **The usual bottleneck.** Native blocks where the microcode was captured, interpreter elsewhere |
| GS (VIF1 unpack, GIF, rasterization) | GS worker + GPU | GPU-bound at high internal resolution |
| Presentation | main | Scaling shader, FXAA, vsync |
| IOP | IOP thread | Interpreted code plus native IRX modules; matters for audio and disc load |

Two structural facts drive most numbers:

- **Capture coverage is the VU1 performance.** Every optimization pass converts more microcode to native blocks, so a
  build made from a fresh capture is slower than the same build after playing the same area again.
- **The optimization is per area.** A level the port has never seen has no native blocks for its microcode.

## Counters

| Switch | Output | Use |
|---|---|---|
| `GOW_PERF_DIAG=1` + `GOW_PERF_FRAME_PC=0x001837B8` | `[gow-perf] t=… ee_ms=… vu_ms=… gs_ms=… upload_ms=… guest_flip_hz=… host_hz=…` once per second | Split by subsystem; the launcher always sets these |
| `GOW_DELTA_DIAG=1` | `[gow-speed] speed=… game=… wall=…` | Game time vs real time (1.00 = real). **Check this first** |
| `GOW_VU1_STATS=1`, `GOW_VU1_BUDGET_DIAG=1`, `GOW_VU1N_DIAG=1` | Native block hits/misses, budget cuts | Is the VU1 running native code? |
| `GOW_GS_HW_STATS=1`, `GOW_GS_HW_PROFILE=1` | GS backend counters and per-pass timing | GPU-bound or CPU-bound? |
| `GOW_IO_HIST=<s>`, `GOW_IOP_IO_HIST=<s>` | Hardware registers still touched by game code | Finds work that could become native |
| `GOW_SAMPLING=1` | Periodic samples of the running threads | Feed `ps2-dev sampling` / `sampling-win` for per-function attribution |
| `GOW_PERF_DIAG` + `ps2-dev perf-summary LOG --game` | Windows of 5 s in the loaded game only | Removes menus, loading and transitions from the average |
| `GOW_FRAME_DIAG=1` + `GOW_PERF_FRAME_PC=0x001837B8` | `[gow-frames] p50_ms=… p95_ms=… p99_ms=… max_ms=… over16_7ms=… over33_3ms=…` every 5 s | Frame-time distribution of the game's own frames (the entries `guest_flip_hz` counts): a stutter shows in p99 and `over16_7ms` long before it moves the average. One clock read per frame: cheap enough to keep on during an A/B run. Not presentation latency |
| `PS2X_GS_WORKER_DIAG=1` | `[gs-worker:sync] <site> … waits=… wait_ms=… stage_waits=… stage_wait_ms=…` and `[gs-stage:sync] packets=… push_waits=… wakeups=… idle_ms=… busy_ms=…` | Who waits for whom: the EE for the VU1 worker, the EE for the GS stage alone (worker idle, stage still drawing), the worker for a full stage queue, and how often a push woke a sleeping stage |

With `--game`, `ps2-dev perf-summary` keeps only the windows enclosed by `[gow-pad2:state] state=11 pending=0 levelReady=1`
reports, so menu flips do not pollute the average. Without `--game` it selects windows by time, which is useful for
loading.

## The procedure

1. **Build configuration**: Release, logs off, no LTCG. That is what `install.cmd`/`install.sh` produce; a `-FastDev`
   build is for iterating only.
2. **Environment**: exactly what the launcher gives the game (see [`ENVIRONMENT.md`](ENVIRONMENT.md#set-by-the-launcher)).
   `PS2X_GS_WORKER=1` in particular changes everything: without it the GS runs inline and audio and frame rate collapse.
   `cargo run -p gow-cli -- env` from `gow-launcher/` prints the environment the saved settings would produce.
3. **Scene**: the same save, the same input. A pad script (`GOW_PAD_SCRIPT="5:start,12:x,…"`) or `GOW_PAD_TEST=1`
   makes runs repeatable; a memory card copy makes them safe (`GOW_MC0`). Do not save over the card you use to play.
4. **Warm-up**: skip at least the loading and the first seconds after the level appears.
5. **Windows**: at least 10–15 s of steady state, and report the spread (min/max), not a single second.
6. **Compare like with like**: same scene, same settings, same build options, one change at a time, previous commit
   rebuilt with the same configuration as the baseline.
7. **Sanity**: `speed=1.00`, no other build running, laptop on the performance profile, game window focused.

## Comparing two builds

`scripts/ab-perf.ps1` runs two executables alternately (A B, B A, …) with the launcher's environment, the same disc and the
same pad script, and prints the mean and worst 5 s guest frame rate, p95/p99 and frames over 16.7 ms for each:

```powershell
cargo run -p gow-cli -- env > env.txt     # from gow-launcher/: the environment the saved settings produce
scripts\ab-perf.ps1 -ExeA old\gow-2005.exe -ExeB new\gow-2005.exe -Elf <game>\SCUS_973.99 -EnvFile env.txt `
    -Pairs 4 -Seconds 150 -WarmupSeconds 30 -PadScript '5:start,12:x,18:x,24:x,30:x,36:x'
```

`-Cores 4` pins the game to the first four logical processors, a rough stand-in for a weaker PC (on a CPU with SMT those are two physical cores: Windows numbers the two threads of a core next to each other; `-AffinityMask 0x5` gives a dual core without SMT) (the target of most
of this work); the summary also counts frames over 50 ms (`GOW_FRAMES_HITCH_LOG=1`). Before the pairs the script runs A
once for `-WarmupRunSeconds` (default 90) and discards it: the first run of a series used to meet a cool machine and was
by far the fastest (2 cores: 55-59 FPS against ~40 for the next runs), always in favour of A. Comparing a switch on the same
executable works with `-SetA`/`-SetB`. Early FINISH, measured that way on the ship's combat (3 pairs of 150 s, god mode,
scripted attacks):

| Limit | Off | On |
|---|---|---|
| 120 FPS, all cores | 90.2 FPS, worst window 74.9, p95 13.5 ms | 105.5 FPS, worst window 89.5, p95 11.3 ms |
| 60 FPS, 4 cores | 60.0, p95 17.2 ms, 23.7 frames over 50 ms | 60.0, p95 17.4 ms, 21.0 frames over 50 ms |

The frames over 50 ms in that route were the same with and without it: a pause of ~100 ms each time Kratos hit or was
hit. It was the pad rumble the game asks for on every hit, applied on the EE thread (without a pad: XInput polling of the
empty slots and a new HID scan for a Sony pad); it now runs on its own thread (`ps2_pad_rumble_win.cpp`). After that fix,
one pair at the 120 FPS limit: 0 frames over 50 ms in either variant (from ~27 per session), 99.5 FPS off and 113.3 FPS
on (worst window 77.0 and 103.8). `GOW_STALL_DIAG=1` is how it was found: it samples the main thread's live pc/ra while a
frame is late.

It refuses to start while a `gow-2005` is running and closes only the processes it started (by PID, through their window
first). Logs and `results.json` go to `logs/ab-perf/<date>/`. Only alternating pairs count: the noise between two runs of
the same build is several fps. Both builds must be final builds (see the procedure below), and nothing else may use the
machine while it runs.

### VU1 replay bench

The VU1 alone, without the game's timing noise, through the record/replay of `ps2_vu1_native.inl`:

1. Record once in a combat scene: `GOW_VU1N_RECORD=<file> GOW_VU1N_RECORD_N=4000 GOW_VU1N_RECORD_STRIDE=101
   GOW_VU1N_RECORD_SKIP=3000000` (programs skipped first, so the recording starts in the fight). The file is game data:
   keep it under `logs/` or outside the repository, never in git or a report.
2. Replay with each build: `GOW_VU1N_REPLAY=<file> GOW_VU1N_REPLAY_REPS=6`. The first round checks every record against
   the recorded hash (`[vu1n-replay] N records, M differ`: M must be 0 when the change is meant to be exact); the warm rounds
   only measure and end in `[vu1n-replay] rounds_ms=… median_ms=… min_ms=… max_ms=…`.
3. Compare medians of alternating runs of the two builds. The spread (min to max) says how much a difference must be to
   mean anything; it is typically a few percent, far below the frame-rate noise.

The replay runs on the first VU1 program after start-up, so the game has to boot that far; nothing is drawn or heard.

## Reducing a regression

- If `vu_ms` grew: the VU1 path changed (native block coverage, interpreter, or the capture set).
- If `gs_ms`/`upload_ms` grew: the GS backend or the packet path changed.
- If `ee_ms` grew: a hot override or a syscall got heavier (`getenv` in a hot loop is the classic case; read the
  variables once into statics).
- If the frame time is fine but the window is slow: presentation (scaling, vsync, `GOW_PRESENT_MIN_INTERVAL_US`);
  `GOW_FRAME_PACING_DIAG=1` counts the new frames that reach the screen (`new`, `skipped`).

Profiling per function: `GOW_SAMPLING=1` plus `ps2-dev sampling` (Linux, with `nm`) or `sampling-win` (Windows, with
the linker map: configure with `PS2X_MAP=ON`). The sampler executable itself is built from
`native/tools/profiling/sample.cpp`.

## Benchmarks without the game

Two tools reproduce parts of the frame deterministically:

- `native/tools/render/replay_vif_chain.cpp`: repeats a captured VIF1 chain. With `PS2X_GS_DISCARD_DRAWS=1` and
  `GOW_REPLAY_TIMES=N` it measures the VIF1/VU1 path without rasterization.
- `native/tools/render/replay_gs.cpp`: replays a GS command capture against the CPU/GL backends and compares outputs.
  It is a correctness oracle, not a frame-rate benchmark (it serializes the GS).

## Landmines

- Measuring with logs on (`PS2X_ENABLE_RUNTIME_LOGS=ON`) or with a `-FastDev` build: numbers are not comparable.
- Measuring while another build, another game or a video call is running.
- Trusting the window rate: the window refreshes at the display's rate (144 Hz on the test PC) whatever the game does.
  Use `guest_flip_hz` for the game and `GOW_FRAME_PACING_DIAG=1` for what reaches the screen.
- Comparing FPS limits without `game_s/host_s` (`GOW_HFPS_DIAG=1`): above 60 the game must still run at real-time
  speed (1.00 while playing; loading screens read lower).
- Comparing scenes with different enemy counts: VU1 cost follows what is on screen.
- Forgetting the power profile: on laptops it can halve the frame rate by itself.
