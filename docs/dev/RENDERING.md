# Rendering: the GS layer

The PS2 graphics synthesizer (GS) is implemented as a **host GPU renderer**. There is no VRAM-based software
rasterizer in the shipped path: `GSHwBackend` (`RustPortPS2/native/ps2xRuntime/src/lib/gs/gpu/gs_hw_backend.cpp`)
rasterizes every primitive with OpenGL 4.6 (4.3 as the floor) into RGBA8 render targets, one per frame-buffer base, and keeps the 4 MiB
GS memory image only as the upload/readback source of truth (image transfers, CLUT loads, EE reads, dumps). What
remains of the hardware model is the frontend (`src/lib/gs/gs_frontend.cpp`): register state, GIF tag parsing, vertex
assembly, primitive construction and presentation requests.

**Licensing and origin.** The OpenGL side of the layer (GL context and shared presentation, the threaded backend,
swizzle tables, GS memory layout, backend state, texture page cache, thread priority) is adapted from the SotC port by
Taylor N. Albarnaz / LightVelox ([PS2Recomp `sotc-port`](https://github.com/LightVelox/PS2Recomp/tree/ac9efa070638ad3b3accd284de6f898d5ab271d1),
commit `ac9efa0`), GPL-3.0; those files carry a `// GOW-Port:` header naming the commit. The texture-replacement hashing,
the EE and VU1 interpreters used as references (`src/lib/ee_ref/`, `src/lib/vu_pcsx2/`) and the widescreen patch are
PCSX2-derived, **GPL-3.0+**. Full inventory, per-file licenses and credits: [`../THIRD_PARTY.md`](../THIRD_PARTY.md).
For the environment variables of this layer see [`ENVIRONMENT.md`](ENVIRONMENT.md); for the player-facing controls,
[`../LAUNCHER.md`](../LAUNCHER.md).

## 1. Backends

| Type | Where | Role |
|---|---|---|
| `GSRasterBackend` | `include/runtime/gs/gs_backend.h` | Interface the frontend drives: `Submit`, `LoadClut`, `BeginTransfer`/`UploadImage`, `Flush`, `TextureFlush`, `Sync`, `Present`, `ClearFramebuffer`, `ReadVram`/`WriteVram`, `SnapshotVram` |
| `GSHwBackend` | `src/lib/gs/gpu/gs_hw_backend.cpp` | The renderer: native OpenGL 4.6 render-target path (context ladder down to 4.3, `PS2X_GS_GL_MAX_MINOR`), texture decode + CLUT, channel shuffles, CRTC composition, texture replacement |
| `GSThreadedBackend` | `src/lib/gs/gs_threaded_backend.cpp` | Queue/worker wrapper around any inner backend: serializes commands, replays the producer's MXCSR per command, coalesces identical draw states |
| `GSTextureStore` | `src/lib/gs/gpu/gs_texture_store.{h,cpp}` | Native texture/CLUT store: uploaded bytes at their GS block addresses, CLD rules, palettes, transfers. No rasteriser |
| `GSMem` / `GSSwizzle` | `src/lib/gs/ps2_gs_memory.cpp`, `gs_swizzle.{h,cpp}` | The 13 PSMs' page/block/column addressing. Shared by the store, the renderer, the replacement hashing and the tests |

The backend cannot be switched: `GS::GS()` installs `GSThreadedBackend::MakeDefault()`, which is
`GSThreadedBackend(GSHwBackend)`. `GS::setRasterBackend(nullptr)` (host shutdown) drains the queue and destroys the GL
context on its own thread. When no GL context can be created, the backend logs `[gs-hw] GL unavailable: …` and keeps
working as a no-op rasterizer, while VRAM/CLUT/readback calls still work through the store. This is the mode the C++
tests run in on a machine without a GPU. There is no CPU rasterizer, no compute backend and no backend-selection
switch: `GSCpuBackend`, `GSGpuBackend` and `PS2X_GS_GPU`/`PS2X_GS_THREAD` are not in the tree. The `replay_gs` tool
still names those types, so that target does not build (§5).

Switches that do exist, in the shipped path:

| Variable | Effect |
|---|---|
| `PS2X_GS_HW_SCALE=<n>` | Internal render scale, float, clamped to `>= 1` (§2) |
| `PS2X_GS_DISCARD_DRAWS=1` | `Submit` becomes a no-op (frontend-only runs) |
| `PS2X_GS_COMPACT_QUEUE=0` | Disables draw-state coalescing in the queue |
| `PS2X_GS_QUEUE_CHUNKS=<n>` | Pending 64 KiB blocks before a new `Present` is skipped (default 200, max 256) |
| `PS2X_GS_DIRECT_PRESENT=0` | Forces RAM presentation instead of the shared texture |

## 2. Frame path

```
EE -> DMA (VIF1) -> VIF1 unpack -> VU1 (XGKICK) -> GifArbiter (PATH1/2/3)
   -> GS::processGIFPacket -> GSRasterBackend (GSHwBackend, on the GS thread)
   -> Present (shared texture, or a RAM frame) -> raylib window
```

**VIF1 and GIF.** `PS2Memory::deliverDma` hands VIF1 and GIF chains to the GS worker (`PS2X_GS_WORKER=1`, the play
configuration) or decodes them inline. The worker runs VIF1 unpack and VU1, and VU1 emits packet batches through
`GifArbiter`, which keeps FIFO order inside each PATH: PATH1 first, PATH2 `DIRECTHL` waiting for an `IMAGE` at the
head of PATH3, a normal `DIRECT` keeping its priority over PATH3. Each PATH keeps its own GIF cursor, so an incomplete
tag, `REGLIST` padding or `IMAGE` payload may continue across DMA blocks without being re-read as a tag. `FLG=3` is
consumed as `IMAGE2`, like PCSX2's `GSState::Transfer`.

**Threads.** With `PS2X_GS_WORKER=1` the worker owns VIF1, VU1 and the GIF paths; `PS2X_GS_PIPELINE` (on by default
with the worker) gives the GS frontend its own stage thread, so VU1 of packet *N+1* overlaps the frontend work of
packet *N*. The stage queue is bounded at 4 MiB and is part of the worker for synchronization: `PS2Memory`'s
worker-idle wait also waits for the stage, so every EE access that synchronizes with the worker sees all queued GS work
done. DMA completion is still signalled at the kick. `PS2X_GS_PIPELINE=0` keeps the frontend on the worker thread.

**FINISH.** The frontend's `GS_REG_FINISH` flushes and calls `Sync(GSSyncReason::Finish)`, then sets the CSR FINISH bit.
With `GOW_GS_FINISH_ASYNC=1` (launcher default) the flush and the `glFinish` are dropped and the `Sync` is only queued:
the EE sees the bit immediately, and the reads that need real data (EE VRAM reads, presentation, dumps) synchronize
on their own through the queue. Without it, FINISH waits for the GPU.

**Early FINISH** (on with the worker since 0.3.2; `GOW_GS_EARLY_FINISH=0` turns it off). God of War ends each frame's
VIF1 chain with a PATH2 A+D write to FINISH and waits for its interrupt before it builds the next frame, so the worker sat
idle between frames. `PS2Memory::deliverDma` scans every VIF1 packet for those writes (`countPath2Finish`, skipping UNPACK
data, MPG code and DIRECTs cut by the packet end) and sets CSR.FINISH when the packet is queued, or when the worker starts
it if another job is waiting, so the EE is at most one job ahead. The worker then queues markers on GIF path 0 for the
stage (`GS::stageMarker`): how many of the job's PATH2 FINISH writes the game already received (the frontend skips those),
0 at the job's end (a count the job did not use never swallows the next job's FINISH), and the display registers
(DISPFB/DISPLAY/PMODE), which the game now writes through the queue so they take effect in drawing order, with a shadow
copy for its read-modify-write. CSR is read and acknowledged without waiting for the worker. A/B on the ship's combat
(same executable, 3 alternating pairs of 150 s): 90.2 → 105.5 FPS (+17%) and worst 5 s window 74.9 → 89.5 at the 120
FPS limit; no difference at the 60 FPS limit on 4 cores of the test PC, where both stay at 60. The test dumps of VRAM
(`GOW_PAD_CAPTURES`) can catch the next frame being drawn, because the EE is ahead; the presented window does not change.

**Presentation.** `GSHwBackend::Present` composes the enabled CRTC circuits (1 over 2 or `BGCOLOR`, blended by `ALP`),
cutting the picture to the game's 448-line TV area, and either publishes it as a shared GL texture
(`GSSharedPresent`, the default; no readback) or reads it back for `GSHostPresentation::prepare` to upload.
`GSSharedPresent::Acquire` shows completed frames in order, one per window refresh, jumping to the newest only when
more than three are queued; `GOW_PRESENT_MIN_INTERVAL_US` (default 15000; the launcher sets 80 % of a frame at the FPS limit, 13333 at 60, so that
60 FPS still advances every second refresh of a 144 Hz display)
keeps a burst from being shown faster than real time, and `GOW_SHARED_WAIT_US` (0 by default) bounds the wait for a
frame that is microseconds from completing. `GOW_PRESENT_FIFO=0` selects "show the newest frame" instead.
`GSThreadedBackend::Present` skips the presentation (the window repeats the last frame) when the queue is full;
`GOW_PRESENT_BLOCKING=1` waits instead.

A display-register glitch filter in `Present` holds the read circuits' registers when they change after 30 stable
presents and commits the new value one present later, which removes the one-frame stale image at the start of the
opening movie's clips; `GOW_DISPFB_FILTER=0` disables it. The last presentation can be captured with `GOW_HOST_SHOT=<n>`
or `GOW_HOST_SHOT_ON_CHANGE=<k>`.

**Scaling and the shader pass.** The window image is drawn by raylib with one fragment shader
(`gs_host_present.cpp`): FXAA over the game's texels when `PC_FXAA=1`; otherwise a 5-tap Catmull-Rom bicubic plus a
contrast-adaptive sharpen (`GOW_SHARPEN`, default 0.5) when the frame is magnified, and a single linear tap when the
frame is already at least as large as its destination. The sharpen is a mix of the bicubic result with its four direct
neighbours, not RCAS: the launcher's `PC_SHARPNESS` is not read by the runtime, which uses `GOW_SHARPEN`.
`GOW_SCALER=bilinear` (and the launcher's "Bilinear"/"Pixel" modes) bypass the bicubic/sharpen path: with FXAA off the
texture is drawn with a plain bilinear stretch, and with `PC_FXAA=1` only the FXAA branch of the same shader runs.

**Internal resolution.** `PS2X_GS_HW_SCALE` scales the render targets (`ceil(pxW * scale)` × `ceil(pxH * scale)`) and
the CRTC composition; presentation publishes the scaled image and the host shader fits it to the window. Geometry is
shifted by half a *host* pixel (`0.5 / scale`) so that GS pixel `x` lands on the center of host pixel `scale * x`;
sprites are not shifted, because they are 2D blits whose planes must read the same texel. The launcher clamps the value
to 1–6 and writes both `PS2X_GS_SCALE` and `PS2X_GS_HW_SCALE`, but only the latter is read by the runtime.

**Aspect and Hor+ widescreen.** `PC_ASPECT` decides the shape of the presented picture (`auto`, `stretch`, `4:3`,
`16:9`, `21:9`, `32:9`, …) and `gowPresentationAspect()` falls back to 4:3 with side bars when the widescreen switch is off.
`PC_WIDESCREEN=<aspect>` turns on PCSX2's "Widescreen 16:9" patch: on every VBlank `gowWidescreenVBlank()` writes the
aspect into the game's data (`0x32F1B0`, `0x2FD930`, `0x2FD968`, flag `0x29CC90`) and then the **projection factor** as a
float at `0x7F00` plus the video flag at `0x7F04`; `0x7F00` is what the game's projection code loads
(`lwc1 $f1, 0x7F00`). The factor is `0.75`, or `1/A` while the menu background is showing; during a movie the patch
keeps the game's own 16:9 mode and only sets the flag at `0x7F04`. `PC_WIDESCREEN_304D94=1` also writes
`0x304D94 = 2` for comparison. The parsing and the accepted range (`4:3 < aspect <= 4.0`) live in
`src/lib/gow_aspect.h`, tested without the game by `native/tests/gow_aspect_test.cpp`.

**32:9 (5120×1440) is experimental.** Nothing in the runtime is specific to it: F stays `0.75` and the camera's field of
view is scaled by `F * A`, which at 32:9 is `8/3` against `7/4` at 21:9 and `4/3` at 16:9. Reading the generated code
(`sub_00169238`, called by `sub_001697F0` before `sub_001696C8`), the projection matrices and the values that
`sub_001696C8` derives at camera `+0x38C..+0x398` are computed from the same widened field at camera `+0x37C`, so the
game's own frustum should widen with the picture; there is no separate hard-coded width limit in that path. That has
**not** been confirmed in game: objects appearing or disappearing near the edges, LOD changes and full-screen effects at
such a wide angle still need to be looked at. Known costs that are not bugs of the patch: the HUD and 2D screens are
stretched across the whole width (see Limits); the 3D scene is rendered at the PS2 frame's 4:3 pixel grid times the
internal scale and stretched to the window, so the horizontal detail per degree drops as the picture widens (3× or more
is advisable at 32:9); and with the menu background showing, `F = 1/A` crops the picture vertically, which at 32:9 keeps
only about 37 % of the original height.

**Frame-rate limits.** `PC_GAME_FPS=30` caps the game at 30. `PC_LOGIC_FPS=120` (unset by default; the project never runs
above 120, `kMaxFps` in `lib/gow_frame.h`, and a higher value gets 120) runs the game logic at that rate
(`src/game/gow_logic_rate.cpp`: the frame end waits for 1/N s slots of guest time and the time step floor becomes 60/N
fields, so the game keeps real-time speed); the launcher's FPS limit (60 / 120) sets it. With it, `GSHostPresentation::prepare` latches a new picture on each game buffer swap
(`gowGameFlipCount`, counted by the display setup) and on the VBlank only after two VBlanks without a swap; at 60 it
latches on each VBlank. `PC_WINDOW_FPS` (60 by default, clamped to 30–120; 0, which used to mean unlimited,
gives 120) sets the host loop's `SetTargetFPS`; the launcher passes 60 with the 60 limit (the loop then follows the game's VBlank, `hostSyncToVBlank`) and the display's
refresh rate, at most 120, with the 120 one. `PC_VSYNC=1` requests the vsync hint instead, honoured only on a display of 120 Hz
or less (`applyWindowVsync`): with it the timer follows the display, above that the window follows the game's VBlank.

## 3. Texture replacement

`GOW_TEXTURE_REPLACE=<folder>` indexes a PCSX2-format pack (recursively, by file name). Names are PCSX2's:

```
<texture>[-<clut>][-r<w>x<h>]-<bits>.png|dds
```

| Part | Computed from |
|---|---|
| `texture` | XXH3-64 of the texture's 256-byte blocks as GS memory holds them, hashed a block row at a time. For 24/16-bit formats, indexed formats stored in the high bits, and textures smaller than a block, it is the XXH3-64 of the texels expanded to RGBA or to one index byte per texel |
| `clut` | XXH3-64 of the 16 or 256 palette entries as RGBA words (after TEXA); only for indexed formats |
| `r<w>x<h>` | Size of the part `CLAMP`'s region modes read; such textures are **not** replaced |
| `bits` | `PSM \| TW << 6 \| TH << 10` as 8 hex digits (bit 14, TCC, is ignored), plus TEXA's `TA0 << 15`, `AEM << 23`, `TA1 << 24` for 24/16-bit formats |

The name is computed lazily, once per (TEX0, TEXA, CLAMP, CLUT) key, and again after a write touches the texture's
blocks; the pack's `wantsBits` test runs first so a texture no file can match is never hashed. Files are indexed at the
first draw and decoded on background threads (PNG through stb_image, DDS/BC through bcdec); the renderer keeps drawing
the game's own texture until the replacement arrives, unless `GOW_TEXTURE_REPLACE_SYNC=1`, which waits up to 5 s.
Uploads are mip-mapped (DDS levels, or `glGenerateMipmap` for a single level) and the shader keeps normalizing
coordinates by the *original* texture size, so a replacement of any resolution fits.

Budgets: 768 MB of decoded images on the CPU (`GOW_TEXTURE_REPLACE_CPU_MB`, least recently drawn dropped first),
1536 MB of textures on the GPU (`GOW_TEXTURE_REPLACE_GPU_MB`), and at most 48 MB uploaded per 16 ms window
(`GOW_TEXTURE_REPLACE_UPLOAD_MB`). `GOW_TEXTURE_REPLACE_DIAG=1` logs the computed name and `replacement` / `no file` for
the first 400 textures, plus each upload, under the `[gow-texrepl]` tag. Format, entry points and what is not replaced:
[`../TEXTURES.md`](../TEXTURES.md).

## 4. GS correctness work that is structural

- **Swizzle.** `GSSwizzle` builds forward page tables per PSM from `GSMem` and addresses a texel as
  `page << pixelsPerPageShift | table[entry]`, byte and bit offset included; 4-bit formats keep the other nibble and
  CT24/Z24 writes preserve VRAM's high byte. The same tables are uploaded once as an `R16UI` texture for the shaders,
  plus the inverse tables the channel-shuffle path needs.
- **Texture page cache.** A single 8 KiB page is materialized on first read (`GSMem::TexturePageCache`,
  `GSSwizzle::TextureCache`) and invalidated by a write to the page or by `TEXFLUSH`; the state structure a replay would
  round-trip carries that page and the pending CT24 pixel, but the shipped backend does not export it.
- **CLUT.** 512 halfwords (CT32 uses the low and high halves as separate 256-entry palettes, CT16 uses all 512),
  indexed by `CSM1`/`CSM2`, `CSA`, `CBP` and `TEXCLUT`; the load respects `TEX0.CLD` and can read its source from a
  render target. Its hash is part of the cache key of both the decoded texture and the palette texture.
- **Texture cache.** Decoded RGBA8 textures and index+palette textures live per (TEX0, TEXA, CLUT), with a generation
  per color page: a write invalidates exactly the entries whose blocks it touched. Only level 0 of the game's texture
  is sampled (`MIPTBP1/2` are kept in the context but not used); replacements may bring their own levels.
- **Reinterpretation.** The game uploads data in one format and samples it in another. Palette and depth aliases, plus
  CT16 views over CT32/CT24 targets, are handled by a dedicated 8×8-block shuffle program instead of sampling the RGBA
  target at the wrong coordinates.
- **Geometry.** Vertices are `xy - XYOFFSET/16` (`XYOFFSET` keeps its four fractional bits) and Z is masked to the ZBUF
  format (`Z32` all 32 bits, `Z24` 24, `Z16` 16) and normalized by that format's maximum. Lines become quads one PS2
  pixel wide around the segment; a sprite takes its UVs from the min/max of both vertices, so an inverted axis samples
  the same texels, and a seam it shares with the previous sprite of the batch is left open instead of clamping.
- **Targets, Z and alpha.** One RGBA8 target per frame-buffer base (`FBW·64` × 512 GS pixels) with a 32F depth
  texture attached for the draw's Z buffer base; `ZTST`, `ZMSK`, `ATST`/`AREF`/`AFAIL` (including the per-bit
  framebuffer/depth write masks), `DATE`/`DATM` and the per-bit `FBMSK` are applied per primitive. A Z buffer that
  lives in the target's own color memory (`ZBP == FBP`) is tested and written through the shader's Z-alias path, and
  the shadow passes that invert Z are tracked per Z base. `COLCLAMP=0` and the blends OpenGL cannot express with fixed
  factors (`C = Ad`, `1 + C`) are evaluated in the fragment shader, which reads the destination through a separate
  sample; batches that need it are marked as soft blends.
- **No software fallback.** A primitive or a view the renderer cannot express is dropped with a counted reason
  (`[gs-hw-skip]`, dumped with `GOW_GS_HW_DUMP`), not rasterized somewhere else.

## 7. Internal scale below 1x and effect switches

- **Below 1x.** `PS2X_GS_HW_SCALE` accepts 0.25 to the GPU's limit (the launcher offers 0.25, 0.5 and 0.75 for integrated
  GPUs). Every size in the backend already follows the float `scale` (render targets are `ceil(px * scale)`, scissors, vertex
  offsets, presentation), and the scaler stretches the picture to the window. Checked on a Radeon RX 6600: 0.75x is clean;
  **0.5x shows a tinted rectangle across the centre of bright, bloom-heavy scenes** (the altar light), which `GOW_GS_RT_SHIFT=0` does not
  remove, so it is not the filtered-read shift. Cause not found; the launcher says so next to the 0.5x and 0.25x options.
- **Effect switches** (`gs_frontend.cpp`, Performance tab): a masked primitive is never submitted, and the next draw rebuilds
  its state (`m_drawStateDirty`). Shadow volumes are untextured, `COLCLAMP` off, `ALPHA = 0x68`; additive 3D effects are
  `ALPHA` with a = 0, b = 2, d = 1 and the Z test on (the HUD and screen flashes keep it off); the game's full-screen present copy is never
  masked. `GOW_FX_FILTER=0` forces nearest sampling except for that copy. Checked at the Athens altar: with the switches off the
  light beam and glow disappear and the rest of the picture is the same.

## 5. Verification tooling

| Tool | What it does | What it proves (and what it does not) |
|---|---|---|
| `ps2x_tests` (`RustPortPS2/native/ps2xTest`) | Runs the GS frontend, the texture store, transfers and register behavior. Where no GL context is available the renderer degenerates to a no-op and the VRAM/CLUT/state assertions still run | Register, GIF and VRAM semantics; **not** rasterization. `RustPortPS2/native/tests/known_failures.txt` lists the known failures |
| `replay_vif_chain` (`tools/render/`, `EXCLUDE_FROM_ALL`) | Replays one frame's VIF1 chain over VIF1/VU1/GS without the EE, prints ms per frame and a hash of everything that reaches the GS (`GOW_REPLAY_HASHES` adds a per-launch VU1 state hash) | Isolates VIF1/VU1/GS from the EE: with PCSX2's packets, a correct image rules them out. Inputs (`vram.bin`, `vu1MicroMem.bin`, `vu1Memory.bin`, `vif1_stream.bin`, optional `eeHwRegs.bin`) come from `ps2-dev vif-extract` or `GOW_VIF_CAPTURE`; they are game data and stay local |
| `GOW_GS_VERIFY=<n>` | Draws every n-th batch twice in-process (fast shader vs the full reference program) from the same target state and compares color and depth | Self-consistency of the two shader paths; not agreement with the PS2 |
| `ps2-dev gif-inspect` / `ps2-dev vif-extract` | Structural metrics of local GIF captures (tag sizes, XYZ/ADC counts, XYZ ranges, per-file SHA-256); VIF1 dump extraction | Packet structure and stream shape; it does not rebuild GS state or prove a visible triangle |
| `ps2-dev check-gs-replay EXECUTABLE CAPTURE.bin` | Drives the CLI of `gow_gs_replay` (`replay_gs.cpp`): argument validation, `--checkpoints-sync`, `--until-record`, limits, CPU reproduction of a `GOWGSR1` capture | The tool's own CLI contract only. It links `GSCpuBackend`/`GSGpuBackend`, which are not in the tree, so the target is **not part of the build** and its GS parity numbers are not usable |
| `PS2X_EE_DIFF=<addr,…>\|all` (`ee_ref`) | Re-runs the first `PS2X_EE_DIFF_MAX` calls of a recompiled function on PCSX2's EE interpreter from an entry snapshot and prints every differing register or written word | EE/FPU/MMI semantics, meaning the data *before* the GS; says nothing about rasterization |
| `GOW_VU1_PCSX2=1` (`vu_pcsx2`) | Runs VU1 on PCSX2's interpreter instead of the compiled blocks | An A/B for the microcode interpreter and for packet production; covers VU1, not the GS |
| Backend counters (`GOW_GS_HW_STATS`, `GOW_GS_HW_PROFILE`, `GOW_GS_GPU_TIMER`, `GOW_GS_INVENTORY=1`, `GOW_GS_QUEUE_DIAG`) | Draw/batch/upload counters, per-class GPU timestamps, a census of feedback draws, depth-as-texture, CLUT-from-target and VRAM accessors, queue and wait times | Where the GPU time and the difficult primitives go; the timers wait for query results and change the game's pace, so they are not an FPS measurement |

## 6. Limits and unknowns

- **Hor+ stretches the HUD.** Only the projection factor changes; the 2D HUD keeps its 4:3 layout inside the wider
  view and is therefore stretched with it. The stretch grows with the aspect: 4/3 times as wide at 16:9, about 1.75 times
  at 21:9 and about 2.67 times at 32:9, where the corners of the HUD sit far from the centre of the screen.
- **CLAMP-region textures are never replaced** (`-r<w>x<h>` names are generated for packs, but the runtime skips
  them), nor is anything the game samples back out of a render target (`src`/`rsrc`/`zsrc` draws).
- **No PS2 cache model.** There is no 8 KiB texture cache, no page-validity table and no access-order model for a
  primitive that reads the pages it writes: a draw that samples its own target is merely ordered with
  `glTextureBarrier`, so it sees that point of the batch rather than a frozen PS2 texture-cache snapshot. There is no
  per-primitive `PS2X_GS_FEEDBACK_SNAPSHOT` policy either.
- **Batch-level aliasing only.** Color/depth aliases are separated by completing a batch, not per pixel or per
  primitive.
- **Points are not rasterized.** The frontend builds them; the backend counts them as skipped primitives.
- **PABE is decoded but not applied.** The frontend stores `GS_REG_PABE` in the draw state; the renderer does not use
  it, so a pass-through blend is rendered as a normal blend.
- **Mip levels of the game's textures are ignored** (level 0 only); only replacement packs contribute levels.
- **"Pixel" upscaling is a bilinear stretch** in this runtime; the bicubic/sharpen/FXAA shader is the only scaler.
- **Presentation is capped at the 448-line TV area**, so a taller display mode is cropped.
- **The Linux GS context is a GLFW context** (the library raylib uses). When sharing with the window fails, the runtime
  logs `GLX/EGL sharing failed; using RAM presentation` and presents through a readback.
- **Approximate figures.** The per-class GPU costs recorded during development (triangles, sprites, loads/dumps,
  decoding) were single-machine measurements; treat them as orders of magnitude, not as targets.
