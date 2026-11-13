# Memory card

God of War uses `MC2_D.IRX` on top of `DBCMAN.IRX` / `SIO2D.IRX`. The port does **not** emulate the SIO2 chip or the
card protocol: the card is an image file in the format other emulators already use (PCSX2 `.ps2`, 8 MB, with ECC), and
the IOP services around it are native code.

## Where the card is

| Container | Resolution |
|---|---|
| IOP side (`mc2_d`) | `GOW_MC0` / `GOW_MC1`, defaulting to `Mcd001.ps2` next to the ELF (`iop_emulator.cpp`); the file the launcher shows in **Files → Memory card** |
| EE side (`mc0:` paths) | `<ELF dir>/mc0` |

`GOW_MC0=0` leaves slot 1 empty; `GOW_MC1` is only read when set, so slot 2 is disconnected by default. Saves go to the
IOP container, which is why the 8 MB `Mcd001.ps2` is the file to back up. The `mc0/` folder only holds the few
`mc0:` file operations the game performs outside the card image.

## What the implementation does

| File | Role |
|---|---|
| `ps2xIOP/src/emulator/core/memory_card.{h,cpp}` | Card image, directory and FAT access, ECC |
| `ps2xIOP/src/native/iop_native_overrides.cpp` | `MC2_D` card I/O and `SIO2D` device detection as native code |
| `ps2xRuntime/src/lib/Kernel/Stubs/MemoryCard.cpp` | EE-side libmc2 entry points |
| `ps2xRuntime/src/lib/Kernel/Syscalls/Helpers/Path.h` | `mc0:` / `mc1:` path mapping |

Four IOP behaviors are required for the game to accept a card:

1. **`sceSifSetDmaIntr` (sifman 32) must complete a transfer and fire its callback.** `dbcman` answers the EE with it
   and waits for the end-of-DMA callback (which releases a semaphore) before sending the next answer. It copies like
   `sceSifSetDma`, schedules the callback as an interrupt and is also used by `smpd`.
2. **`WaitEventFlag` without a current thread must wait** (`KE_EVF_COND`). `mc2_d` performs the SIO2 transfers from
   `dbcman`'s RPC server and `sio2man` waits for each one; as with `WaitSema`, the other IOP threads run until the
   condition is met, instead of the call returning immediately and the answer being read before it arrives.
3. **Consecutive `LWL`/`LWR` on the same register combine.** On the R3000 the second one uses the value still in the
   load delay slot of the first. `mc2_d` copies the card specifications (page, block and card size) this way.
4. **The IOP's `sceCdReadClock` (cdvdman 24) must exist** and return BCD time in Japan time (UTC+9), like a retail console.
   `mc2_d` uses it for save dates; the game subtracts 9 hours and adds the configured time zone, which the runtime
   takes from the host, so saves show local time.

Regression tests for each of them live in `ps2xTest/src/ps2_iop_tests.cpp`.

## Verified

With a copy of a PCSX2 card that holds a save, the load menu lists saves with thumbnail, area, date, difficulty and
play time, and loading ends in "Load Complete" at the save point. Saving (including overwriting) ends in "Save Complete",
the file system stays valid and the port loads the new save afterwards.

## Creating and protecting the card

- `ps2-tools mc-format` (`crates/ps2-tools/src/memcard.rs`) writes a formatted, empty card: superblock, indirect FAT
  cluster 8, FAT clusters 9–40, root directory at cluster 41 with `.` and `..`. The superblock, the indirect FAT cluster and
  the FAT match a card the PS2 BIOS formatted in PCSX2 (`GOW_TEST_PCSX2_CARD=<card> cargo test -p ps2-tools` compares
  them). The installers run it when `Mcd001.ps2` does not exist.
- `memory_card.cpp` creates a missing card through `<card>.tmp` and a rename, copies an existing card to `<card>.bak`
  (through a temporary too) before the first write of each session, and moves a card of the wrong size to
  `<card>.unusable` instead of writing pages over it. `native/tests/memory_card_test.cpp` covers the three.

## Save history

The card is written page by page and nothing at that level says a save is finished, so the port watches the game's own save
state machine instead. God of War saves through libmc2 (EE) over `MC2_D`/`DBCMAN`: `0x22FE30` writes the slot's payload with
the whole-file writer `0x22E800` and then the 68-byte slot index with the same writer, called from `0x2306B8`. The writer is
polled once per frame; it returns 1 when the transfer ended, with the game's error code at `0x2A1284` and the byte count at
`0x2A128C`. `src/game/gow_save.cpp` wraps it and, when the call that returns to `0x2306C0` (slot 0, 68 bytes) comes back with
1, no error and 68 bytes, the save is complete: one `[gow-save] save complete` line and a dated copy of the card.

- **Where**: `<card folder>/save-backups/<YYYYMMDD-HHMMSS>[-N]/`, with `Mcd001.ps2` (published last: its presence means the
  copy is complete) and `backup.json` (`created`, `created_unix`, `source`, `size`). `src/lib/gow_save_backup.cpp` writes it,
  `gow-launcher/crates/gow-core/src/saves.rs` reads it.
- **How many**: the newest 30 automatic copies are kept (`GOW_SAVE_BACKUP_KEEP`); manual copies and folders the port did not
  write are never removed.
- **The card is never touched** by this; the copy is made from the file the IOP already wrote through. A failed copy is a notice
  on screen and a `[gow-save] backup failed` line, and the game carries on.
- **Checked in game** (Windows, a private copy of a PCSX2 card): load at the Fortress of Athens altar, save, "Save Complete"; one
  copy, byte-identical to the card afterwards, the original card unchanged.
- **Restoring** is done from the launcher's Saves tab with the game closed (the runtime keeps the image in memory): the card it
  replaces goes to the history first (`source: before-restore`).

The same signal is a fixed point to correlate the open hang after saving with (`STATUS.md`): the animation loop that spins
afterwards runs after this call returned.

## The slot index and what the game shows of a save

The file `BASCUS-97399GodOfWar` inside the folder of the same name is the game's slot index (68 bytes): four 16-byte records
and a trailing word. The game's save code writes each record at `0x230634..0x230670`:

| Offset | Type | Value | Source in RAM |
|---|---|---|---|
| +0 | u32 | payload number (`data<N>.bin`); `0xFFFFFFFF` = empty slot | `0x35BC60[card]` |
| +4 | u32 | `0xCAFEBAD1` | |
| +8 | f32 | play time in seconds (the game shows `h:mm:ss`) | `0x302D0C` |
| +12 | u16 | save point (the altar) | `0x2A122C` |
| +14 | u8 | difficulty, 0 to 3, easiest first | `0x29CC9C` |

The game keeps the index of each card it read at `0x35BE80` (0x44 bytes per card). The payload `data<N>.bin` (80 KiB) starts
with `0xCAFEBAD1` and the zone name at +8. The load screen passes the save points to `0x1B7AA8`, which looks up the texture
`TXR_SavePt%02d` (format at `0x2B55D0`) for each thumbnail.

On the disc (the player's own; nothing of it is in the repository): `GODOFWAR.TOC` is a list of 24-byte entries
`[char name[12], u32 part, u32 size, u32 sector]` (part 0 = `PART1.PAK`, 1 = `PART2.PAK`, the file at `sector * 2048`).
`R_PERM.WAD` holds the message bank `msgs_en.txt`: the save point `n` is named by message `600 + n` (601 to 641 on the retail
disc) and the difficulty `d` by message `3 + d`. `R_SAVEPT.WAD` holds, for each save point, `TXR_SavePt<NN>` (names of its
pixels and palette at +4 and +28), `GFX_SavePt<NN>` (24-byte header, then 128x80 8-bit indices in the GS PSMT8 layout) and
`PAL_SavePt<NN>` (24-byte header, 256 RGBA colours in CSM1 order, alpha `0x80` = opaque). WAD records are
`[u32 tag][u32 size][char name[24]]` plus the payload, aligned to 16 (see `NATIVE_MENU_RESEARCH.md`).
`gow-launcher/crates/gow-core/src/save_art.rs` reads all of it from the extracted files or the ISO.

**Checked**: on a copy of a PCSX2 card the first record is `0, 0xCAFEBAD1, 2902.96 s, 4, 0`, and the game's own load screen
shows that slot as the save point 4 altar, the easiest difficulty and `0:48:22`, the same as the launcher's page; the
thumbnail decoded from the retail disc shows the same picture the game draws for that slot (compared by eye).

## Loading a slot from outside the game's menus

`src/game/gow_save_load.cpp` takes a request (the launcher's `<card folder>/load-request` file, the F2 menu or
`GOW_LOAD_SLOT=<0-3>`) and walks the path a player takes, writing only what the game's own menus write at each step:

1. **Gameplay or pause** (main state `0x29E560` = 11 or 12, nothing pending at `0x29E574`): one gameplay frame (`0x21F3C8`)
   or pause frame (`0x21F638`) is replaced by `0x21D868`, the pause menu's Quit (it stores 0 at UI `+0x1A8` first, as the
   pause menu does). `0x21E9D0` resets the session and sets the main state to 1.
2. **Title screen** (state 3 while `0x29CC54`, the "main menu loaded" flag read by `0x1BBCF8`, is 0): a short Start press
   in the packet of the pad read, every half second.
3. **Main menu** (state 3, `0x21EBF8`): right before its frame, UI `+0x278` = 2 (Load) and `+0x27C` = 1 (a choice was
   made), the variables its own menu sets (`*0x29CB94` holds the UI variables). The frame opens the load screen (state 5).
   `0x29E574` stays set while the main menu is up, so it is not a "busy" flag there.
4. **Load screen** (`0x22F5E0`, state word `0x2A12B0`, jump table `0x2B8500`): state 8 waits for the player's pick in
   `0x2A1290` (-1 while none, 0 back, slot + 1) for the card `0x2A12A4`. The slot is written there once the game's own
   index says it is in use; the game then reads `data<N>.bin`, applies it (play time to `0x302D0C`) and starts gameplay.

Each step has a deadline (120 s for the first, which includes the intro at start), and a request that cannot go on ends
with a notice; an empty slot is refused before leaving the game. The request file is taken with a rename and ignored when
older than ten minutes. The F2 menu's Saves page lists the index the game holds in memory with names from its own
message bank (`*0x29CBA0` index, `*0x29CBA4` lines, `0x29CBA8` count).

**Checked in game** (Windows, private copies of a PCSX2 card, `GOW_SAVE_DIAG=1`):

- At start (`GOW_LOAD_SLOT=0`, and a request file written before the game opened): title, main menu, load screen, slot;
  gameplay at the slot's altar, play time of that slot.
- During gameplay: a request for the fourth slot went back to the main menu and loaded it (play time `0:48:26` against
  `0:48:22` of the first slot); a request for an empty slot was refused without leaving the game.
- With the pause menu open: back to the main menu from state 12 and the first slot loaded.
- The F2 Saves page lists both slots with the game's names, difficulty and play time.
- No hang, crash or bad-call line in any of those sessions.

**Not checked**: the F2 page driven by a real keyboard or gamepad (only its drawing and the shared request path were run),
a request during a movie, a level load or the save screen (the request waits for gameplay, the pause menu or the main menu
and gives up after its deadline), the second card slot, Linux.

## Not verified

- **The game formatting a blank card.** The installer avoids it by creating the card formatted.
- **PCSX2 opening a card written by the port** (the reverse direction) was not checked recently.
- Card sizes other than 8 MB, and the second slot.

User-facing behavior (how to copy your card, what to back up) is in [`../PLAYING.md`](../PLAYING.md#saves-and-the-memory-card).
