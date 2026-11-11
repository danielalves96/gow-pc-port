//! `iop-manifest`: the IOP module dump (`PS2X_IOP_DUMP_MODULES`) rebuilt from the disc's own IRX files, without opening the game.
//!
//! The runtime attaches a module's native code (`iop-gen`) when the module's relocated text has the base, size and FNV hash
//! recorded in `iopn_registry.cpp`. The text of an IRX at a given load base is a pure function of the IRX file (a MIPS ELF
//! with relocations): the runtime loader (`ps2xIOP/src/emulator/services/iop_module_loader.cpp`) copies the segments into the
//! IOP RAM and applies the relocations, and `attachNative` reads the text right after that, before the module's start
//! function ran. This module repeats that load in a 2 MiB buffer and writes what the dump writes: `<ident>.bin` (the text
//! range) and a `modules.txt` line `<ident> <base hex> <text size hex> <fnv hash hex>`.
//!
//! The committed manifest (`games/scus-97399/iop.manifest`) holds no game bytes: per module the IRX file name on the disc,
//! the physical load base the game gets (module arena order) and the expected text size and hash. A module whose rebuilt
//! text does not match is not written (another disc revision), and `iop-gen` then simply has fewer modules.
//!
//! ```text
//! V 1
//! M <IRX file> <load base hex> <text size hex> <fnv hash hex>
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use crate::hash::fnv;

const VERSION: u32 = 1;
/// `IopMemory::RamSize`.
const RAM_SIZE: usize = 2 * 1024 * 1024;
/// `IopMemory::HeapBase`: modules must end below it.
const HEAP_BASE: u32 = 0x0007_0000;
const LOAD_BASE: u32 = 0x0001_0000;

const ET_EXEC: u16 = 2;
const ET_SCE_IOPRELEXEC: u16 = 0xFF80;
const ET_SCE_IOPRELEXEC2: u16 = 0xFF81;
const PT_LOAD: u32 = 1;
const PT_SCE_IOPMOD: u32 = 0x7000_0080;
const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;
const SHF_ALLOC: u32 = 2;

/// One manifest line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub file: String,
    pub base: u32,
    pub text_size: usize,
    pub hash: u64,
}

impl Entry {
    /// The module's identifier as the runtime derives it from the IRX name: lowercase, non-alphanumerics as `_`.
    pub fn ident(&self) -> String {
        self.file.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
    }
}

pub fn parse_manifest(text: &str) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<Entry> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let number = index + 1;
        match fields[0] {
            "V" => {
                if fields.get(1).and_then(|v| v.parse::<u32>().ok()) != Some(VERSION) {
                    return Err(format!("iop.manifest:{number}: unsupported version"));
                }
            }
            "M" => {
                if fields.len() != 5 {
                    return Err(format!("iop.manifest:{number}: expected `M FILE BASE SIZE HASH`"));
                }
                let hex =
                    |s: &str| u64::from_str_radix(s, 16).map_err(|_| format!("iop.manifest:{number}: bad number {s}"));
                let entry = Entry {
                    file: fields[1].to_owned(),
                    base: u32::try_from(hex(fields[2])?).map_err(|_| format!("iop.manifest:{number}: base too big"))?,
                    text_size: usize::try_from(hex(fields[3])?)
                        .map_err(|_| format!("iop.manifest:{number}: bad size"))?,
                    hash: hex(fields[4])?,
                };
                if entries.iter().any(|e| e.ident() == entry.ident()) {
                    return Err(format!("iop.manifest:{number}: {} is listed twice", entry.file));
                }
                entries.push(entry);
            }
            other => return Err(format!("iop.manifest:{number}: unknown record {other}")),
        }
    }
    Ok(entries)
}

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn align_up(value: u32, alignment: u32) -> u32 {
    if alignment <= 1 {
        value
    } else {
        value.wrapping_add(alignment - 1) & !(alignment - 1)
    }
}

fn in_range(total: usize, offset: u32, size: u32) -> bool {
    offset as usize <= total && size as usize <= total - offset as usize
}

struct Phdr {
    kind: u32,
    offset: u32,
    vaddr: u32,
    filesz: u32,
    memsz: u32,
}

struct Shdr {
    kind: u32,
    flags: u32,
    addr: u32,
    offset: u32,
    size: u32,
    link: u32,
    info: u32,
    entsize: u32,
}

/// The loaded module: its RAM image and what the runtime's `IopImageLoadResult` holds that the dump needs.
struct Loaded {
    ram: Vec<u8>,
    text_size: usize,
}

fn read_u32_ram(ram: &[u8], address: u32) -> u32 {
    u32_at(ram, address as usize).unwrap_or(0)
}

fn write_u32_ram(ram: &mut [u8], address: u32, value: u32) {
    if let Some(slot) = ram.get_mut(address as usize..address as usize + 4) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

/// `IopModuleLoader::load` with the load base given instead of taken from the module cursor.
fn load_irx(image: &[u8], requested_base: u32) -> Result<Loaded, String> {
    if image.len() < 52 || &image[..4] != b"\x7fELF" || image[4] != 1 || image[5] != 1 || u16_at(image, 18) != Some(8) {
        return Err("not a 32-bit little-endian MIPS ELF".to_owned());
    }
    let e_type = u16_at(image, 16).unwrap_or(0);
    let phoff = u32_at(image, 28).unwrap_or(0);
    let shoff = u32_at(image, 32).unwrap_or(0);
    let phentsize = u32::from(u16_at(image, 42).unwrap_or(0));
    let phnum = u32::from(u16_at(image, 44).unwrap_or(0));
    let shentsize = u32::from(u16_at(image, 46).unwrap_or(0));
    let shnum = u32::from(u16_at(image, 48).unwrap_or(0));

    let (mut min_vaddr, mut max_vaddr, mut has_load) = (u32::MAX, 0u32, false);
    let mut programs = Vec::new();
    if phnum != 0 && phentsize >= 32 && in_range(image.len(), phoff, phentsize * phnum) {
        for i in 0..phnum {
            let at = (phoff + i * phentsize) as usize;
            let program = Phdr {
                kind: u32_at(image, at).unwrap_or(0),
                offset: u32_at(image, at + 4).unwrap_or(0),
                vaddr: u32_at(image, at + 8).unwrap_or(0),
                filesz: u32_at(image, at + 16).unwrap_or(0),
                memsz: u32_at(image, at + 20).unwrap_or(0),
            };
            if program.kind == PT_LOAD && program.memsz != 0 {
                has_load = true;
                min_vaddr = min_vaddr.min(program.vaddr);
                max_vaddr = max_vaddr.max(program.vaddr.wrapping_add(program.memsz));
            }
            programs.push(program);
        }
    }
    let mut sections: Vec<Shdr> = Vec::new();
    if shnum != 0 && shentsize >= 40 && in_range(image.len(), shoff, shentsize * shnum) {
        for i in 0..shnum {
            let at = (shoff + i * shentsize) as usize;
            let section = Shdr {
                kind: u32_at(image, at + 4).unwrap_or(0),
                flags: u32_at(image, at + 8).unwrap_or(0),
                addr: u32_at(image, at + 12).unwrap_or(0),
                offset: u32_at(image, at + 16).unwrap_or(0),
                size: u32_at(image, at + 20).unwrap_or(0),
                link: u32_at(image, at + 24).unwrap_or(0),
                info: u32_at(image, at + 28).unwrap_or(0),
                entsize: u32_at(image, at + 36).unwrap_or(0),
            };
            if !has_load && section.flags & SHF_ALLOC != 0 && section.size != 0 {
                min_vaddr = min_vaddr.min(section.addr);
                max_vaddr = max_vaddr.max(section.addr.wrapping_add(section.size));
            }
            sections.push(section);
        }
    }

    if min_vaddr == u32::MAX {
        min_vaddr = 0;
    }
    let span = if max_vaddr > min_vaddr { max_vaddr - min_vaddr } else { 0x1000 };
    let span = align_up(span, 0x100);
    let relocate = e_type != ET_EXEC || max_vaddr as usize > RAM_SIZE || (min_vaddr < LOAD_BASE && min_vaddr != 0);
    let (base, delta) = if relocate {
        if u64::from(requested_base) + u64::from(span) >= u64::from(HEAP_BASE) {
            return Err(format!("module at 0x{requested_base:x} does not fit the module arena"));
        }
        (requested_base, i64::from(requested_base) - i64::from(min_vaddr))
    } else {
        if min_vaddr != requested_base {
            return Err(format!("module is not relocatable and sits at 0x{min_vaddr:x}, not 0x{requested_base:x}"));
        }
        (min_vaddr, 0)
    };
    let shifted = |value: u32| (i64::from(value) + delta) as u32;

    let mut ram = vec![0u8; RAM_SIZE];
    let section_addr: Vec<u32> = sections.iter().map(|s| s.addr).collect();
    if has_load {
        for program in &programs {
            if program.kind != PT_LOAD || program.memsz == 0 {
                continue;
            }
            if !in_range(image.len(), program.offset, program.filesz) || program.memsz < program.filesz {
                return Err("segment outside the file".to_owned());
            }
            let destination = shifted(program.vaddr) as usize;
            if destination >= RAM_SIZE || program.memsz as usize > RAM_SIZE - destination {
                return Err("segment outside the IOP RAM".to_owned());
            }
            let source = &image[program.offset as usize..(program.offset + program.filesz) as usize];
            ram[destination..destination + source.len()].copy_from_slice(source);
            // The rest up to memsz is already zero.
        }
    } else {
        for (index, section) in sections.iter().enumerate() {
            if section.flags & SHF_ALLOC == 0 || section.size == 0 {
                continue;
            }
            let destination = if section.addr != 0 {
                shifted(section.addr)
            } else {
                // The runtime loader would place it after the previous section; no IRX of the game needs that.
                return Err(format!("section {index} has no address; not supported"));
            };
            if destination as usize >= RAM_SIZE || section.size as usize > RAM_SIZE - destination as usize {
                return Err("section outside the IOP RAM".to_owned());
            }
            if section.kind != SHT_NOBITS {
                if !in_range(image.len(), section.offset, section.size) {
                    return Err("section outside the file".to_owned());
                }
                let source = &image[section.offset as usize..(section.offset + section.size) as usize];
                ram[destination as usize..destination as usize + source.len()].copy_from_slice(source);
            }
        }
    }

    let is_iop_relocatable = e_type == ET_SCE_IOPRELEXEC || e_type == ET_SCE_IOPRELEXEC2;
    apply_relocations(image, &sections, &section_addr, delta, base, is_iop_relocatable, &mut ram);

    // Text size from the PT_SCE_IOPMOD header (`entry gp textsize` after the module-info pointer), else the module span.
    let mut declared = 0usize;
    for program in &programs {
        if program.kind == PT_SCE_IOPMOD && program.filesz >= 12 && in_range(image.len(), program.offset, 12) {
            if program.filesz >= 16 {
                declared = u32_at(image, program.offset as usize + 12).unwrap_or(0) as usize;
            }
            break;
        }
    }
    let text_size = if declared != 0 && declared <= span as usize { declared } else { span as usize };
    Ok(Loaded { ram, text_size })
}

fn apply_relocations(
    image: &[u8],
    sections: &[Shdr],
    section_addr: &[u32],
    delta: i64,
    load_base: u32,
    is_iop_relocatable: bool,
    ram: &mut [u8],
) {
    struct PendingHi16 {
        address: u32,
        symbol_value: u32,
        symbol_index: u32,
    }
    let mut hi16: Vec<PendingHi16> = Vec::new();
    for relsec in sections {
        if relsec.kind != SHT_REL && relsec.kind != SHT_RELA {
            continue;
        }
        let Some(&target_addr) = section_addr.get(relsec.info as usize) else { continue };
        let target_base = (i64::from(target_addr) + delta) as u32;

        // (value, shndx) of every symbol of the linked symbol table.
        let mut symbols: Vec<(u32, u16)> = Vec::new();
        if let Some(symsec) = sections.get(relsec.link as usize) {
            if symsec.kind == SHT_SYMTAB && symsec.entsize >= 16 && in_range(image.len(), symsec.offset, symsec.size) {
                for i in 0..(symsec.size / symsec.entsize) as usize {
                    let at = symsec.offset as usize + i * symsec.entsize as usize;
                    symbols.push((u32_at(image, at + 4).unwrap_or(0), u16_at(image, at + 14).unwrap_or(0)));
                }
            }
        }

        let entry_size = if relsec.kind == SHT_RELA { relsec.entsize.max(12) } else { relsec.entsize.max(8) };
        if entry_size == 0 || !in_range(image.len(), relsec.offset, relsec.size) {
            continue;
        }
        let mut offset = 0u32;
        while u64::from(offset) + u64::from(entry_size) <= u64::from(relsec.size) {
            let at = (relsec.offset + offset) as usize;
            let relocation_offset = u32_at(image, at).unwrap_or(0);
            let relocation_info = u32_at(image, at + 4).unwrap_or(0);
            let explicit_addend = if relsec.kind == SHT_RELA { u32_at(image, at + 8).unwrap_or(0) as i32 } else { 0 };
            offset += entry_size;

            let kind = relocation_info & 0xFF;
            let symbol_index = relocation_info >> 8;
            let mut symbol_value = if is_iop_relocatable { load_base } else { 0 };
            if let Some(&(value, shndx)) = symbols.get(symbol_index as usize) {
                if !is_iop_relocatable || symbol_index != 0 {
                    symbol_value = value;
                    if shndx != 0 {
                        symbol_value = (i64::from(symbol_value) + delta) as u32;
                    }
                }
            }
            let place64 = if is_iop_relocatable {
                u64::from(load_base) + u64::from(relocation_offset)
            } else {
                u64::from(target_base) + u64::from(relocation_offset)
            };
            if place64 > u64::from(u32::MAX) {
                continue;
            }
            let place = place64 as u32;
            if u64::from(place) + 3 >= RAM_SIZE as u64 {
                continue;
            }
            let word = read_u32_ram(ram, place);
            let addend = if relsec.kind == SHT_RELA { explicit_addend } else { word as i32 };
            match kind {
                2 | 3 => write_u32_ram(ram, place, (i64::from(addend) + i64::from(symbol_value)) as u32),
                4 => {
                    let target = ((word & 0x03FF_FFFF) << 2).wrapping_add(symbol_value);
                    write_u32_ram(ram, place, (word & 0xFC00_0000) | ((target >> 2) & 0x03FF_FFFF));
                }
                5 => hi16.push(PendingHi16 { address: place, symbol_value, symbol_index }),
                6 => {
                    let lo = i32::from(word as u16 as i16);
                    let mut index = 0;
                    while index < hi16.len() {
                        if hi16[index].symbol_index != symbol_index {
                            index += 1;
                            continue;
                        }
                        let pending = hi16.remove(index);
                        let hi_word = read_u32_ram(ram, pending.address);
                        let hi = i64::from(i32::from(hi_word as u16 as i16) << 16);
                        let full = hi + i64::from(lo) + i64::from(pending.symbol_value);
                        let relocated_hi = (((full + 0x8000) >> 16) as u32) & 0xFFFF;
                        write_u32_ram(ram, pending.address, (hi_word & 0xFFFF_0000) | relocated_hi);
                    }
                    let full = i64::from(lo) + i64::from(symbol_value);
                    write_u32_ram(ram, place, (word & 0xFFFF_0000) | ((full as u32) & 0xFFFF));
                }
                1 => write_u32_ram(
                    ram,
                    place,
                    (word & 0xFFFF_0000) | ((addend.wrapping_add(symbol_value as i32) as u32) & 0xFFFF),
                ),
                _ => {}
            }
        }
    }
}

/// The text of `image` relocated at `entry.base`, checked against the manifest's size and hash.
pub fn module_text(image: &[u8], entry: &Entry) -> Result<Vec<u8>, String> {
    let loaded = load_irx(image, entry.base)?;
    let end = entry.base as usize + loaded.text_size;
    if loaded.text_size != entry.text_size || end > loaded.ram.len() {
        return Err(format!("text size 0x{:x}, the manifest says 0x{:x}", loaded.text_size, entry.text_size));
    }
    let text = loaded.ram[entry.base as usize..end].to_vec();
    if fnv(&text) != entry.hash {
        return Err(format!("text hash 0x{:x}, the manifest says 0x{:x}", fnv(&text), entry.hash));
    }
    Ok(text)
}

/// A file of `game` by name, ignoring case, in the root or in `IOP_MOD`.
fn find_irx(game: &Path, name: &str) -> Option<PathBuf> {
    for folder in [game.to_path_buf(), game.join("IOP_MOD")] {
        let Ok(listing) = fs::read_dir(&folder) else { continue };
        for item in listing.flatten() {
            if item.file_name().to_string_lossy().eq_ignore_ascii_case(name) && item.path().is_file() {
                return Some(item.path());
            }
        }
    }
    None
}

/// `iop-manifest apply GAME MANIFEST OUTDIR`: writes `<ident>.bin` and `modules.txt` like `PS2X_IOP_DUMP_MODULES`.
pub fn apply_manifest(game: &Path, manifest: &Path, output: &Path) -> Result<i32, String> {
    let text = fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let entries = parse_manifest(&text)?;
    fs::create_dir_all(output).map_err(|e| format!("{}: {e}", output.display()))?;
    let mut listing = String::new();
    let mut written = 0;
    for entry in &entries {
        let Some(path) = find_irx(game, &entry.file) else {
            eprintln!("[iop-manifest] {}: not found in {}", entry.file, game.display());
            continue;
        };
        let image = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        match module_text(&image, entry) {
            Ok(code) => {
                let target = output.join(format!("{}.bin", entry.ident()));
                fs::write(&target, &code).map_err(|e| format!("{}: {e}", target.display()))?;
                listing.push_str(&format!(
                    "{} {:x} {:x} {:x}\n",
                    entry.ident(),
                    entry.base,
                    entry.text_size,
                    entry.hash
                ));
                written += 1;
                println!("{}: base 0x{:x}, text 0x{:x}", entry.ident(), entry.base, entry.text_size);
            }
            Err(message) => eprintln!("[iop-manifest] {}: {message}; skipped", entry.file),
        }
    }
    let target = output.join("modules.txt");
    fs::write(&target, listing).map_err(|e| format!("{}: {e}", target.display()))?;
    println!("modules written: {written} of {}", entries.len());
    Ok(if written > 0 { 0 } else { 1 })
}

/// `iop-manifest verify GAME MANIFEST DUMPDIR`: every module of a real dump equals what the manifest rebuilds. Developer check.
pub fn verify_manifest(game: &Path, manifest: &Path, dump: &Path) -> Result<i32, String> {
    let text = fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let entries = parse_manifest(&text)?;
    let mut mismatches = 0;
    for entry in &entries {
        let path = find_irx(game, &entry.file).ok_or_else(|| format!("{}: not found", entry.file))?;
        let image = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let rebuilt = module_text(&image, entry);
        let dumped =
            fs::read(dump.join(format!("{}.bin", entry.ident()))).map_err(|e| format!("{}: {e}", entry.ident()))?;
        let same = rebuilt.as_ref().is_ok_and(|code| *code == dumped);
        println!("{}: {}", entry.ident(), if same { "identical" } else { "DIFFERENT" });
        if !same {
            mismatches += 1;
        }
    }
    Ok(i32::from(mismatches > 0))
}
