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

## Not verified

- **The game formatting a blank card.** The installer avoids it by creating the card formatted.
- **PCSX2 opening a card written by the port** (the reverse direction) was not checked recently.
- Card sizes other than 8 MB, and the second slot.

User-facing behavior (how to copy your card, what to back up) is in [`../PLAYING.md`](../PLAYING.md#saves-and-the-memory-card).
