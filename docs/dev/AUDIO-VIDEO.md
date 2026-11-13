# Audio and video (FMV)

The sound unit that LIBSD drives and the MPEG decoder behind `sceMpeg*` are host implementations; everything above them
is recompiled guest code. See [`ENVIRONMENT.md`](ENVIRONMENT.md) and [`../PLAYING.md`](../PLAYING.md).

## Audio path

### Native boundary: LIBSD, not SPU2

There is no SPU2 model: no register file, no SPU2 DMA channels, no memory-mapped sound RAM. Sample transfers are
copies into a 2 MB buffer owned by the engine.

God of War's IOP runs the original IRX modules (`989NOMID.IRX`, `SMPD_IOP.IRX`), compiled ahead of time to native code
that is used only when they load at the expected base with the same text hash (`ps2xIOP/src/native/iop_native.h`). The driver
reaches the sound unit **only** through LIBSD's exports, native C++ in `ps2xIOP/src/native/iop_native_overrides.cpp`
and listed with the same offsets in `ps2xIOP/native_overrides.txt` for the generator (offsets from the module base;
entries are `(id << 8) | (voice << 1) | core`, bit 7 marking core-level parameters, addresses as byte addresses stored
as halfwords):

- all `sceSd*`: `Init` 0x456c, `SetParam`/`GetParam` 0x3670/0x36cc, `SetSwitch`/`GetSwitch` 0x372c/0x3764, `SetAddr`/
  `GetAddr` 0x379c/0x3800, `SetCoreAttr` 0x3cbc, `VoiceTrans` 0x18a0, `VoiceTransStatus` 0x1af0, `BlockTrans` 0x1c30,
  `BlockTransStatus` 0x1e4c, `SetTransIntrHandler` 0x3e3c, `SetEffectAttr` 0x0c3c, `SetEffectWorkArea` 0x0984; the
  SPU2 bus setup at 0x1560 is a no-op.

### The sound engine

`ps2xIOP/src/emulator/core/sound_engine.{h,cpp}` holds typed state: two cores of 24 voices (L/R volume, pitch, ADSR1/2,
addresses, envelope), VMIX/NON/PMON masks, MMIX, attribute, master/effect/aux/stream volumes, ESA/EEA.

- One stereo sample every 768 IOP cycles (36.864 MHz / 48 kHz). The DSP follows PCSX2 and psx-spx: 4-bit ADPCM, pitch in 4.12
  with linear interpolation, ADSR, a dry mix per voice through VMIX and the voice volume, then the core master volumes.
- `NAX` reads back inside the block being played (PCSX2's `NextA`), so 989snd's polled streams are not aborted.
- The core input stream (`sceSdBlockTrans`) reads 1 KB blocks of 256 left then 256 right samples from the IOP ring and
  mixes them at `BVOL` (a plain signed 16-bit level) while MMIX's input bits (0x80/0x40) are set; mode 2 stops,
  `SD_BLOCK_LOOP` (0x10) loops. `sceSdBlockTransStatus` returns the address read (bits 0-23) and the half (bit 24).
- Transfers complete at once, but `VoiceTrans` rounds the size up to 64 bytes and, in DMA mode, schedules the driver's
  `handler(channel, arg)` 512 IOP cycles later through `IopNativeServices::callGuestLater`, like the DMA interrupt.

The sample clock is the IOP cycles the EE granted (`IopEmulator::runCycles`), clamped to each grant; work outside a grant
(EE-side RPC into IOP servers, module starts) advances the clock without producing samples, which `GOW_IOP_DRIFT` reports.

### Ring buffer and output device

- `step()` writes into a lock-free single-producer ring of 2^17 stereo frames (2.7 s): the producer is the EE thread and
  the consumer the host audio thread, which never waits; frames written with nobody consuming count as overflow.
- `drainOutput` keeps `kSpu2MaxLatencyFrames` (6000 frames, 125 ms, `ps2_runtime.cpp`) and absorbs the clock
  difference with a WSOLA stretcher (Hann grains of 512 frames at 50 % overlap, splice within ±256 frames): tempo 1.0
  inside a -5 %..+20 % dead band, else 0.85x-1.10x at 0.6 % per grain, so the pitch never changes.
- A dry ring fades the last grain and rebuilds the buffer; a buffer 0.3 s above the target is cut back once; the
  callback pads leftover frames with silence. A peak limiter closes the mix (instant drop, ~80 ms return), disabled by
  `GOW_AUDIO_LIMIT=0`.
- There is no resampling stage: raylib's stream (1024-frame buffer) or miniaudio on a chosen device
  (`ps2_audio_device.cpp`) opens at the engine rate, 16-bit stereo 48 kHz. `GOW_AUDIO_DEVICE` accepts part of a device
  name (`default` = system default); otherwise the first line of `%LOCALAPPDATA%\GodOfWarNative\audio_device.txt`
  decides. If nothing matches, the runtime lists the devices and falls back to the default.

### Diagnostics

- `GOW_AUDIO_STATS=1`: `[audio-stats]` every ~5 s, reporting frames requested/missing, starvation, cut-backs, tempo and level
  ranges, overflow, `over_full_scale` (the mix itself, not the output path), limiter depth and duration.
- `GOW_IOP_DRIFT=1`: `[iop-drift]` granted vs advanced IOP cycles and the five addresses that spent most cycles
  outside `runCycles`.
- `GOW_SND_REGLOG=1`: `[snd-reg]` for core-level writes (MMIX, master/effect/aux/stream volumes). `GOW_SND_DIAG=1`
  logs the core input stream calls, and `GOW_AUDIO_RAW=<file>` writes the mix before rate adaptation.
- `GOW_SND_VOICE_DIAG=1`: `[gow-voices]` every 5 s of mixed audio, per core: voices taken (`active`), in release, in release
  with an envelope below 0x2000 (`release_low`, the voices a stalled release would leave taken) and the most taken at the end
  of any engine step (`peak_active`). After a fight `active` and `release_low` should fall back to the voices that still sound.
  `RustPortPS2/native/tools/audio/sound_engine_regression.cpp` checks the envelope and mix cases without the game.

## Video path (FMV)

### Decoder

The `sceMpeg*` entry points are recompiler stubs (`RustPortPS2/games/scus-97399/gow-port.toml`) implemented in
`ps2xRuntime/src/lib/Kernel/Stubs/MPEG.cpp`. MPEG-2 is decoded by the vendored **libmpeg2 0.5.1**
(`ps2xRuntime/third_party/libmpeg2`, plain C, GPL-2.0-or-later, target `libmpeg2`); FFmpeg is an opt-in alternative
only (`-DPS2X_ENABLE_FFMPEG=ON`, default OFF).

The stub demultiplexes the PSS: pack and system headers, the M2V stream (type 0) into the decoder, private-stream-1
audio (PCM type 2, ADPCM type 3) to the game's callbacks. Video is fed only after a sequence header is found, so the one
the game demuxes before `sceMpegCreate` is not lost; libmpeg2 returns display-order RGBA (BT.601 to RGB) and
`sceMpegGetPicture` writes it as RGB32 strips of 16x16 macroblocks, at most `mbcount` x 1024 bytes (0 for `.m2`).

Two libmpeg behaviors are kept because the game depends on them:

- Stream callbacks run synchronously inside the demux call on the demuxing thread, the no-data callback on the thread
  that requests the picture. `sceMpegAddStrCallback` replaces the callback of the same stream (accumulating callbacks
  delivered every audio packet twice); `sceMpegCreate` keeps input demuxed before it and clears the callback table.
- Audio packets refused by the game's ring are queued in order and offered first on the next demux; the queue is
  dropped when a new stream starts on the same `sceMpeg`, which the game reuses for every movie.

The video ends at its sequence end code (`00 00 01 B7`) or at the PSS program end (`00 00 01 B9`), whichever is first;
audio after it is still delivered. Once the video has ended and every decoded picture was handed out, the stub writes
the word the game's own `sceMpegIsEnd` (`0x26FF70`) reads, so the player leaves its playing state.

The IPU is a stub (`Kernel/Stubs/IPU.cpp`): `sceIpuInit` writes the IQ/VQ tables into the IPU register window and
reports success; `sceIpuRestartDMA`, `sceIpuStopDMA` and `sceIpuSync` return 0. Nothing is decoded or filtered there.

### The movie player and the GS

The runtime produces the picture; the game's own movie player (`0x188848`) fetches each picture (`0x18A170`), then
uploads and draws it as a sprite. The port overrides only what blocks playback (`ps2xRuntime/src/game/gow_movie.cpp`):
the video ring's write pointer is moved to the base when it sits at the end (the two are the same position, but the
player read the end as data), and the stream poll counts host time already spent as guest time (`EeScheduler::catchUpToHost`, one field
per call), so the movie thread does not fall to ~0.72x real time.

**End of a movie.** The game reads a movie through a stream (`0x2FDE10`: `+0x28` state, 3 once the file reached its end; `+0x24`
the 16 KB blocks still queued) and the read callback `0x188848` hands the blocks out. When none are left it returns 0, and the
refill `0x18AC20` takes 0 for "nothing yet": in its blocking path it calls the callback again, for ever; only a negative result
stores the read-EOF flag (`movie+0x154`). `gowMovieStreamRead` answers -1 for that call (from `0x18AC70` or `0x18AD34`, size
positive, stream at its end, nothing queued, no cancel flag `0x29C848`). The completion test `0x189B00` returns 1 as soon as
the flag is set, and the decoder could still hold pictures, so `gowMovieCompletion` waits for the decoder's reference buffer
(`*(*(movie+0x40))`, what `0x26FF70` reads) to be written by the MPEG stub, for at most 1.5 s. The drain loop after the last
picture (`0x189794`) calls the audio refill `0x189A88` without the stream service's host-time catch-up, so
`gowMovieTailAudio` applies the same one-field catch-up at that call site. `GOW_MOVIE_EOF=0` turns the first two off.
Checked: `P04MP05` (the Cliffs of Madness lever movie, 741 pictures) plays to its end and returns to the game with and without
the change in this runtime, so the freeze reported against another build of the port was **not reproduced here**; the
wrappers only act when the stream is drained and the flag is set, which did not happen in that run. They are kept because
they are the cause that build's reporter and the disassembly point to, not because a failure here was fixed.

Widescreen needs no decoder handling: the game asks for a name without extension and loads the `.PSW` variant where the
disc has one. With widescreen on, the port's only part is the VBlank hook (`gow_integration.cpp`): while a movie loads or
plays (states 3-9 and 11 of `0x29C838`) it keeps `0x304D94` at its original value and writes `0x7F04 = 1`.

### A/V synchronization

Picture timing comes from the MPEG PTS and is driven by VSync: the first handed-out picture defines the base tick, each
next one is due at `base + delta(90 kHz) x 2/3003` fields. A picture that is already late is shown at once; only a stall of 30
VSyncs (0.5 s) rebases the clock, so a late GOP start does not push the rest back; one due in the future is held with
`waitVSync`.

Audio is not slaved to the picture: it is produced on the emulated IOP clock and reaches the device through the
stretcher. `GOW_MPEG_DIAG` prints `[MPEG:sync]` every 30 pictures with PTS, wall time, device frames played and
produced and the decoder time; that line measures the residual audio-behind-picture offset.

### Debug switches

- `GOW_SKIP_FMV=1` replaces the movie API (`0x188CA0` load, `0x188E80` start, `0x188ED8`/`0x188EF0` ready, `0x188F20`)
  with versions that return at once, and logs `[gow-fmv] skipped <name>`; the player overrides are not installed.
- `GOW_MOVIE_OVERRIDE=<name>[,<name>...]` plays those movies (up to 31 characters each) instead of the loaded ones, one
  per load; with several entries the title-menu hook starts the next once a movie ends.
- `GOW_MPEG_DIAG=1` traces pictures, callback registrations and refusals, held pictures and end of stream; `=2` also
  logs every read of a large stream.

## Limits

- **Reverb, sweeps and interpolation are approximate**: `SetEffectAttr`/`SetEffectWorkArea` only store mode, depths and
  work area; sweep-mode volumes apply their target at once; per-voice changes can step; interpolation is linear, not
  the hardware's 4-point gaussian.
- **No noise generator and no pitch modulation**: the NON and PMON masks are stored, not used.
- **Routing is simplified**: core 0 goes through its master into core 1's dry output, then core 1's master; no effect
  or aux sends.
- **The IPU does not filter**: deblocking and IPU color conversion are not modeled and the IQ/VQ tables are only
  stored; the picture is libmpeg2's IDCT and BT.601 conversion, close to but not identical with the PS2 output.
- **No disc-stream timing model**: the PSS is served as fast as the game reads it, and the end only comes from data the
  demuxer saw, so a file whose last block is never demuxed keeps waiting. Real time comes from the player poll
  accounting host time as guest time, at most one field per call.
- **A/V sync is one-sided**: video follows its PTS, audio follows the emulated clock and is only stretched to the
  device; the residual offset is measured, not corrected, and a starved ring fades and rebuilds.

## Environment variables

| Variable | Effect | File |
|---|---|---|
| `GOW_AUDIO_DEVICE` | Part of an output device name; `default`/empty = system default; no match lists devices | `ps2xRuntime/src/lib/ps2_audio_device.cpp` |
| `GOW_AUDIO_STATS` | `[audio-stats]` every ~5 s: frames, starvation, tempo, level, overflow, limiter | `ps2xIOP/src/emulator/core/sound_engine.cpp` |
| `GOW_AUDIO_LIMIT` | `0` disables the peak limiter (hard clip) | `ps2xIOP/src/emulator/core/sound_engine.cpp` |
| `GOW_AUDIO_RAW` | Writes the mix (s16le, 48 kHz) before rate adaptation | `ps2xIOP/src/emulator/core/sound_engine.cpp` |
| `GOW_IOP_DRIFT` | Granted vs advanced IOP cycles and the top outside calls, every ~5 s | `ps2xIOP/src/emulator/iop_emulator.cpp` |
| `GOW_SND_REGLOG` | Core-level parameter writes (MMIX, master/effect/aux/stream volumes) | `ps2xIOP/src/emulator/core/sound_engine.cpp` |
| `GOW_SND_DIAG` | Core input stream calls with the device position, and core volumes | `ps2xIOP/src/native/iop_native_overrides.cpp` |
| `GOW_SND_VOICE_DIAG` | `[gow-voices]` every 5 s: voices taken, in release, stuck in release, peak per core | `ps2xIOP/src/emulator/core/sound_engine.cpp` |
| `GOW_SKIP_FMV` | `1` replaces the movie API: movies are skipped and the game continues | `ps2xRuntime/src/game/gow_overrides.cpp` |
| `GOW_MOVIE_OVERRIDE` | Movie names played instead of the loaded ones, one per load | `ps2xRuntime/src/game/gow_movie.cpp` |
| `GOW_MPEG_DIAG` | Pictures, callbacks, refusals, `[MPEG:sync]`, loaded names; `=2` all reads | `ps2xRuntime/src/lib/Kernel/Stubs/MPEG.cpp`, `gow_movie.cpp` |

## Where the code is (under `RustPortPS2/native/`)

| Path | What |
|---|---|
| `ps2xIOP/src/emulator/core/sound_engine.{h,cpp}` | Voices, mixing, input stream, ring, stretcher, limiter |
| `ps2xIOP/src/native/iop_native_overrides.cpp` | LIBSD exports as native C++ (`native_overrides.txt` for the generator) |
| `ps2xIOP/src/emulator/iop_emulator.cpp` | Sample clock from the EE grants, `GOW_IOP_DRIFT` |
| `ps2xRuntime/src/lib/Kernel/Stubs/MPEG.cpp` | `sceMpeg*`: PSS demux, libmpeg2 backend, picture timing, diagnostics |
| `ps2xRuntime/src/lib/Kernel/Stubs/IPU.cpp` | `sceIpu*` stubs (tables and success, no decode) |
| `ps2xRuntime/src/game/gow_movie.cpp`, `gow_overrides.cpp` | Skip/override hooks, video-ring handling, host-time catch-up, binding |
| `ps2xRuntime/src/lib/ps2_runtime.cpp`, `ps2_audio_device.cpp` | 48 kHz stream, callback, `kSpu2MaxLatencyFrames`, named device |
| `ps2xRuntime/src/lib/gow_integration.cpp` | Widescreen VBlank hook that keeps `.PSW` movies filling the image |
| `ps2xRuntime/third_party/libmpeg2/` | Vendored decoder, GPL-2.0-or-later ([`../THIRD_PARTY.md`](../THIRD_PARTY.md)); target and FFmpeg option in `ps2xRuntime/CMakeLists.txt` |
