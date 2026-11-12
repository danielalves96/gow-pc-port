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

## Not verified

- **The game formatting a blank card.** The installer avoids it by creating the card formatted.
- **PCSX2 opening a card written by the port** (the reverse direction) was not checked recently.
- Card sizes other than 8 MB, and the second slot.

User-facing behavior (how to copy your card, what to back up) is in [`../PLAYING.md`](../PLAYING.md#saves-and-the-memory-card).
