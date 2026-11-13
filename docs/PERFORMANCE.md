# Performance

## Real-time speed and the FPS limit

*God of War* is built around the PlayStation 2's 60 Hz vertical blank: its logic, animations, physics and scripts tick
60 times per second. The port keeps the game at real-time speed: it never runs faster than on a PS2, and it never runs
in slow motion either. A late frame is dropped, not stretched.

The **FPS limit** in the launcher (60 / 120) is the game's real frame rate. 60 is the PS2's own. At 120 the game logic
itself runs 120 frames per second, each one a shorter step of time, so the game gets smoother without getting faster. A
PC that cannot reach the limit simply shows fewer frames. At the 60 limit the window follows the game's own frames; at 120 it refreshes at the monitor's rate, at most 120.
Nothing runs above 120 FPS: the goal is a steady 60 on any PC, and higher rates only kept the GPU at 100 % (on one AMD
card, until its OpenGL driver crashed). Some actions timed in frames (quick-time events, combo windows) may feel different
above 60: go back to 60 if anything seems off. The counter shows that one real frame rate.

## What the frame rate depends on

| Component | What it does | When it limits |
|---|---|---|
| **EE** (game code) | The recompiled C++ running the game | Rarely alone; it waits on the VU1 and the GS |
| **VU1** (vector unit) | Runs the game's microcode for transforms and effects, on a thread of its own | **Most often.** Combat and busy scenes are VU1-bound |
| **GS** (graphics) | Native OpenGL renderer | At 3× internal resolution and above, on mid-range and integrated GPUs |
| **Presentation** | Scaling shader + window (bicubic/FSR, FXAA, vsync) | When the render is larger than the screen, or at 3×/high refresh |

The rule of thumb: **internal resolution costs GPU, scene complexity costs CPU (VU1)**. Lowering the resolution does
not help a VU1-bound scene; the optimization pass does.

## Measured reference points

These are measurements from development machines, not guarantees. Your numbers will differ; treat them as a sanity
check, not a target.

| Machine | Setting | Result |
|---|---|---|
| Integrated Radeon 680M, laptop on the *performance* power profile | 1×, ship-deck combat | 50–60 FPS |
| Integrated Radeon 680M, same, **power saver** profile | 1× | 20–35 FPS (the CPU clock drops and the VU1 thread with it) |
| Integrated Radeon 680M, launcher defaults (16:10, bicubic + FXAA, 2×, vsync) | 2× | 60 FPS in menus, ~30 in play, iGPU at 90–100 % |
| Discrete RTX 4050 Laptop | 1× / 2× / 3× | 60 / 40–60 / 18–24 FPS |

Two conclusions that hold in every run so far:

- **The power profile matters as much as the settings on a laptop.** A "power saver" profile can halve the frame rate.
- **3× and above is GPU-bound** and can also stall the presentation queue on mid-range cards.

## Tuning, in the order that helps

1. **Render resolution: 2×** (default). Go to 1.5× or 1× on integrated graphics or old GPUs; go up only on a fast
   discrete card, and expect a heavier picture, not a faster game.
2. **Upscaling**: *Bilinear (soft)* is the cheapest; *Bicubic (sharp)* costs a shader pass; *Pixel* is the cheapest and
   sharpest-looking on a native-resolution window.
3. **FXAA**: off if you are fighting for frames, on if edges bother you.
4. **FPS limit**: 60 is the safe choice. 120 makes the CPU and the VU1 do twice the work per second, so it needs a fast
   CPU; heavy scenes fall back to what the PC can reach.
5. **Progressive scan (480p)**: on gives the sharper image; off is the interlaced mode with the anti-flicker filter,
   which is softer but slightly cheaper.
6. **Optimize**: run `-Optimize` / `--optimize` (or let the launcher's background optimization run) after playing new
   areas. Each pass converts more VU1/IOP code into native code, so the game gets faster. It is the only setting
   that relieves a CPU-bound scene.
7. **Close other work**: a background build takes exactly the CPU time the VU1 thread needs.

## Checking that the game runs at real speed

The port ships a game-speed meter: with `GOW_DELTA_DIAG=1` (the launcher already runs with it) the log prints

```
[gow-speed] frames=... wall=... game=... speed=1.00 ...
```

`speed=1.00` means game time equals real time. Above 1 means the game is running *fast* (and an FPS number is
flattering), below 1 means slow motion (and the frame rate is worse than it looks). **Always check `speed` before
trusting an FPS number.**

## Measuring properly

To compare builds or settings:

- Compare the **same scene**, the same settings and the same build configuration, one change at a time.
- Use a **Release** build (`install.cmd` without `-FastDev`, or `./install.sh`). Development builds have logs on and
  are slower; numbers from them do not transfer.
- Do not measure while anything else is compiling, and keep the laptop on the performance profile.
- Read the game's frame rate (the one number in the counter, or `[gow-perf]` lines with `GOW_PERF_DIAG=1`), not the
  window rate: the window always refreshes at the monitor's rate.

The developer-side procedure and the available counters are in [`dev/PERFORMANCE.md`](dev/PERFORMANCE.md).
