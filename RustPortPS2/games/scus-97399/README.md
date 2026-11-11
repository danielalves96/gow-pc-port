# God of War (SCUS-97399): game data

What the recompiler, the installers and the developer tools need to know about this specific game. Nothing here is game
code or content (see "Legal" in `AGENTS.md`).

| File | What it is | Read by |
|---|---|---|
| `gow-port.toml` | `ps2_recomp` configuration: functions the runtime implements (`stubs`), extra `entry_points`, MMIO accesses, the 16:9 instruction patch. Holds the markers `@ELF@`, `@MAP@` and `@OUT@`, which the installers fill in. | `install.ps1`, `install.sh`, `ps2-dev validate-config`, `ps2-config` tests |
| `funcmap.csv` | Function map of the ELF (`Name,Start,End,Size`), exported from Ghidra: 6,414 functions. | `ps2_recomp`, `ps2-tools verify-elf`, `ps2-dev validate-config`, `gow-cli elf-report` |
| `vu1.manifest` | Maps each VU1 micro-memory image to the piece of the ELF it comes from (offsets and hashes only, no code). | `ps2-tools vu1-manifest` / `vu1-programs`, from the installers |
| `iop.manifest` | The IRX modules the game loads: file on the disc, physical load base, text size and FNV-1a hash of the relocated text (no code). `iop-manifest apply` relocates each IRX as the runtime loader does and writes the `PS2X_IOP_DUMP_MODULES` dump that `iop-gen` compiles; a module whose hash differs (another disc revision) is skipped. | `ps2-tools iop-manifest`, from the installers; `ps2-dev validate-config` checks its shape |
| `vu1-heads.list` | Dynamic VU1 entry points (one decimal pc per line) that the native block generator also treats as block starts. | `ps2-tools vu1n-gen --heads` |

Notes:

- `funcmap.csv` stays CSV on purpose. It is Ghidra's export format, changes show up line by line in a diff, and parsing
  all 6,414 rows takes a few milliseconds, once per install. A binary format (Parquet) would make the diffs unreadable
  and add a heavy dependency for no measurable gain; YAML would be bigger and slower.
- The installers copy `vu1-heads.list` as `vu1n_heads.txt` next to the microcode folder (`vu1_captures/`), because
  CMake looks for it there (`PS2X_VU1_MICROCODE_DIR/../vu1n_heads.txt`). Without it, the VU1 blocks it lists are
  interpreted.
- To expand the manifest from a local capture of gameplay, run
  `ps2-tools vu1-manifest extend ELF vu1.manifest captures out.manifest`, then validate the result with
  `vu1-manifest verify`. `extend` accepts only images it can fully recreate from `ELF` and writes offsets, lengths and
  hashes only. Never add the capture files themselves to the repository.
- After editing `gow-port.toml` or `funcmap.csv`, run `ps2-dev validate-config` from `RustPortPS2/`.
- To add a module to `iop.manifest` (another IRX the game loads), take its base, text size and hash from the `modules.txt` of a
  `PS2X_IOP_DUMP_MODULES` dump, then check with `ps2-tools iop-manifest verify GAMEDIR iop.manifest DUMPDIR` that the rebuilt
  text is identical to the dumped one.
