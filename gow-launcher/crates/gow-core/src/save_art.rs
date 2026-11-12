//! What the Saves page shows of a save besides the card: the name of its altar, its difficulty and its thumbnail, read
//! from the player's own installed disc (the extracted files next to the ELF, or the ISO). Nothing of the game is in the
//! repository: without the disc the page falls back to the zone code.
//!
//! Where it is (checked against the retail USA disc, `docs/dev/MEMORY-CARD.md`):
//! - `GODOFWAR.TOC`: 24-byte entries `[char name[12], u32 part, u32 size, u32 sector]`; `part` 0 is `PART1.PAK`, 1
//!   `PART2.PAK`, and the file starts at `sector * 2048` of that part. A name can appear several times (copies placed for
//!   seeking): the first one is used.
//! - A WAD is a list of records `[u32 tag][u32 size][char name[24]]` followed by `size` bytes, the next record at the
//!   next multiple of 16; a record whose tag's low half is `0x18` holds a value in `size` and no payload.
//! - `R_PERM.WAD` holds `msgs_en.txt`, the game's message bank: a line `*N*` starts message N. The save point `n` is
//!   named by message `600 + n`, the difficulty `d` by message `3 + d`.
//! - `R_SAVEPT.WAD` holds `TXR_SavePt<NN>` (the texture: the names of its pixels and its palette at +4 and +28),
//!   `GFX_SavePt<NN>` (a 24-byte header with width and height at +4 and +8, then 8-bit indices laid out as the GS stores
//!   PSMT8) and `PAL_SavePt<NN>` (a 24-byte header, then 256 RGBA colours in the GS CSM1 order, alpha 0x80 = opaque).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Serialize;

const SECTOR: u64 = 2048;
const PLACE_MESSAGE: u32 = 600;
const DIFFICULTY_MESSAGE: u32 = 3;
const DIFFICULTIES: u32 = 4;

/// The names and thumbnails of the save points, ready for the page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveArt {
    /// Save point -> its name as the game writes it.
    pub places: BTreeMap<u16, String>,
    /// Difficulty 0..=3 -> its name.
    pub difficulties: Vec<String>,
    /// Save point -> a `data:image/png;base64,...` of its thumbnail (only those asked for).
    pub thumbnails: BTreeMap<u16, String>,
}

/// One file of the TOC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TocEntry {
    part: u32,
    size: u64,
    sector: u64,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn c_name(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|&b| b as char).collect()
}

fn toc_find(toc: &[u8], name: &str) -> Option<TocEntry> {
    toc.as_chunks::<24>().0.iter().find(|entry| c_name(&entry[..12]).eq_ignore_ascii_case(name)).map(|entry| TocEntry {
        part: u32_at(entry, 12),
        size: u64::from(u32_at(entry, 16)),
        sector: u64::from(u32_at(entry, 20)),
    })
}

/// The files of a WAD: name -> payload.
fn wad_records(wad: &[u8]) -> BTreeMap<String, &[u8]> {
    let mut records = BTreeMap::new();
    let mut at = 0usize;
    while at + 32 <= wad.len() {
        let (tag, size) = (u32_at(wad, at), u32_at(wad, at + 4) as usize);
        let name = c_name(&wad[at + 8..at + 32]);
        at += 32;
        if tag & 0xFFFF != 0x18 && tag != 0 {
            let Some(payload) = wad.get(at..at + size) else { break };
            records.entry(name).or_insert(payload);
            at += size;
        }
        at = (at + 15) & !15;
    }
    records
}

/// The first line of each message of a message bank.
fn messages(text: &[u8]) -> BTreeMap<u32, String> {
    let text: String = text.iter().map(|&b| b as char).collect(); // Latin-1, as the game reads it
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        let Some(id) =
            line.strip_prefix('*').and_then(|rest| rest.strip_suffix('*')).and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(first) = lines.next() {
            out.entry(id).or_insert_with(|| first.trim().to_string());
        }
    }
    out
}

/// GS PSMT8 layout -> plain rows (width a multiple of 16, height of 4).
fn unswizzle8(pixels: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    if width == 0
        || height == 0
        || !width.is_multiple_of(16)
        || !height.is_multiple_of(4)
        || pixels.len() < width * height
    {
        return None;
    }
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let block = (y & !0xF) * width + (x & !0xF) * 2;
            let swap = (((y + 2) >> 2) & 1) * 4;
            let row = ((((y & !3) >> 1) + (y & 1)) & 7) * width * 2;
            let column = ((x + swap) & 7) * 4;
            let byte = ((y >> 1) & 1) + ((x >> 2) & 2);
            out[y * width + x] = *pixels.get(block + row + column + byte)?;
        }
    }
    Some(out)
}

/// The CSM1 palette order swaps entries 8..15 and 16..23 of every 32.
fn csm1(index: u8) -> usize {
    let i = index as usize;
    (i & 0xE7) | ((i & 0x08) << 1) | ((i & 0x10) >> 1)
}

/// A PNG of a save point's thumbnail, from the three records of `R_SAVEPT.WAD`.
fn thumbnail(records: &BTreeMap<String, &[u8]>, point: u16) -> Option<Vec<u8>> {
    let texture = records.get(&format!("TXR_SavePt{point:02}"))?;
    let gfx = records.get(&c_name(texture.get(4..28)?))?;
    let pal = records.get(&c_name(texture.get(28..52)?))?;
    let (width, height) = (u32_at(gfx, 4) as usize, u32_at(gfx, 8) as usize);
    if width > 1024 || height > 1024 || u32_at(gfx, 16) != 8 || pal.len() < 24 + 1024 {
        return None;
    }
    let indices = unswizzle8(gfx.get(24..)?, width, height)?;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for index in indices {
        let colour = &pal[24 + 4 * csm1(index)..24 + 4 * csm1(index) + 4];
        rgba.extend_from_slice(&[colour[0], colour[1], colour[2], (u16::from(colour[3]) * 2).min(255) as u8]);
    }
    let image = image::RgbaImage::from_raw(width as u32, height as u32, rgba)?;
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

/// What a save's names and thumbnails come from, once the two WADs are read.
fn art_from(perm: &[u8], savept: &[u8], points: &[u16]) -> SaveArt {
    let mut art = SaveArt::default();
    if let Some(bank) = wad_records(perm).get("msgs_en.txt") {
        let messages = messages(bank);
        art.places = messages
            .iter()
            .filter(|(id, _)| (PLACE_MESSAGE + 1..PLACE_MESSAGE + 100).contains(*id))
            .map(|(id, text)| ((id - PLACE_MESSAGE) as u16, text.clone()))
            .collect();
        art.difficulties = (0..DIFFICULTIES).map_while(|d| messages.get(&(DIFFICULTY_MESSAGE + d)).cloned()).collect();
    }
    let records = wad_records(savept);
    for &point in points {
        if let Some(png) = thumbnail(&records, point) {
            let encoded = base64::engine::general_purpose::STANDARD.encode(png);
            art.thumbnails.insert(point, format!("data:image/png;base64,{encoded}"));
        }
    }
    art
}

// ---------------------------------------------------------------------------------------------------------------------
// The disc

/// Where the disc's files are: the extracted folder (the files at their names) or an ISO (the files at their sectors).
enum Disc {
    Folder(PathBuf),
    Iso { file: File, files: Vec<ps2_tools::iso::DiscFile> },
}

impl Disc {
    fn open(elf: &str, iso: &str) -> Option<Self> {
        let state = crate::game::check_data(elf, iso);
        if state.extracted {
            return Some(Self::Folder(Path::new(elf).parent()?.to_path_buf()));
        }
        if state.iso.is_empty() || crate::iso::is_chd(Path::new(&state.iso)) {
            return None; // a CHD is not read here: the page shows the zone code
        }
        let mut file = File::open(&state.iso).ok()?;
        let (files, _) = ps2_tools::iso::scan(&mut file).ok()?;
        Some(Self::Iso { file, files })
    }

    fn read(&mut self, name: &str, offset: u64, length: u64) -> Option<Vec<u8>> {
        if length > 64 << 20 {
            return None;
        }
        let mut buffer = vec![0u8; length as usize];
        match self {
            Self::Folder(directory) => {
                let mut file = File::open(directory.join(name)).ok()?;
                file.seek(SeekFrom::Start(offset)).ok()?;
                file.read_exact(&mut buffer).ok()?;
            }
            Self::Iso { file, files } => {
                let entry = files.iter().find(|f| f.name.eq_ignore_ascii_case(name))?;
                if offset + length > entry.size {
                    return None;
                }
                file.seek(SeekFrom::Start(entry.lba * SECTOR + offset)).ok()?;
                file.read_exact(&mut buffer).ok()?;
            }
        }
        Some(buffer)
    }

    fn size(&self, name: &str) -> Option<u64> {
        match self {
            Self::Folder(directory) => std::fs::metadata(directory.join(name)).ok().map(|m| m.len()),
            Self::Iso { files, .. } => files.iter().find(|f| f.name.eq_ignore_ascii_case(name)).map(|f| f.size),
        }
    }

    fn toc_file(&mut self, toc: &[u8], name: &str) -> Option<Vec<u8>> {
        let entry = toc_find(toc, name)?;
        let part = format!("PART{}.PAK", entry.part + 1);
        self.read(&part, entry.sector * SECTOR, entry.size)
    }
}

/// The names of the save points and difficulties, and the thumbnails of `points`, from the installed disc. Empty when
/// the disc cannot be read (no game data yet, a CHD, another layout).
pub fn read_save_art(elf: &str, iso: &str, points: &[u16]) -> SaveArt {
    (|| {
        let mut disc = Disc::open(elf, iso)?;
        let toc_size = disc.size("GODOFWAR.TOC")?.min(1 << 20);
        let toc = disc.read("GODOFWAR.TOC", 0, toc_size)?;
        let perm = disc.toc_file(&toc, "R_PERM.WAD")?;
        let savept = disc.toc_file(&toc, "R_SAVEPT.WAD").unwrap_or_default();
        Some(art_from(&perm, &savept, points))
    })()
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tag: u32, name: &str, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        let mut field = [0u8; 24];
        field[..name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&field);
        out.extend_from_slice(payload);
        while out.len() % 16 != 0 {
            out.push(0);
        }
        out
    }

    /// GS PSMT8 layout of plain rows (the inverse of `unswizzle8`).
    fn swizzle8(plain: &[u8], width: usize, height: usize) -> Vec<u8> {
        let mut out = vec![0u8; width * height];
        for y in 0..height {
            for x in 0..width {
                let block = (y & !0xF) * width + (x & !0xF) * 2;
                let swap = (((y + 2) >> 2) & 1) * 4;
                let row = ((((y & !3) >> 1) + (y & 1)) & 7) * width * 2;
                out[block + row + ((x + swap) & 7) * 4 + ((y >> 1) & 1) + ((x >> 2) & 2)] = plain[y * width + x];
            }
        }
        out
    }

    fn synthetic_wads() -> (Vec<u8>, Vec<u8>) {
        let bank = b"*1*\r\nfirst\r\n\r\n*3*\r\nEasy One\r\n\r\n*4*\r\nNormal One\r\n\r\n*604*\r\nPLACE FOUR\r\n\r\n*605*\r\nPLACE FIVE\r\n";
        let mut perm = record(0x378, "WAD_R_PERM", &[0; 8]);
        perm.extend(record(0x28, "GroupStart", &[]));
        perm.extend(record(0x18, "Value", &[])); // a value record: no payload
        perm.extend(record(0x6F, "msgs_en.txt", bank));
        // a 16x4 thumbnail: index = x, palette entry i = (i, 255 - i, 7, 0x80)
        let (width, height) = (16usize, 4usize);
        let plain: Vec<u8> = (0..height).flat_map(|_| (0..width as u8).collect::<Vec<_>>()).collect();
        let mut gfx = Vec::new();
        for word in [0x0Cu32, width as u32, height as u32, 0, 8, 1] {
            gfx.extend_from_slice(&word.to_le_bytes());
        }
        gfx.extend(swizzle8(&plain, width, height));
        let mut pal = Vec::new();
        for word in [0x0Cu32, 16, 16, 0, 32, 1] {
            pal.extend_from_slice(&word.to_le_bytes());
        }
        for slot in 0..256usize {
            let i = csm1(slot as u8) as u8; // stored in CSM1 order: entry `slot` holds colour csm1(slot)
            pal.extend_from_slice(&[i, 255 - i, 7, 0x80]);
        }
        let mut txr = vec![7, 0, 0, 0];
        let mut name = [0u8; 24];
        name[..12].copy_from_slice(b"GFX_SavePt04");
        txr.extend_from_slice(&name);
        name[..12].copy_from_slice(b"PAL_SavePt04");
        txr.extend_from_slice(&name);
        let mut savept = record(0x3001E, "GFX_SavePt04", &gfx);
        savept.extend(record(0x3001E, "PAL_SavePt04", &pal));
        savept.extend(record(0x1E, "TXR_SavePt04", &txr));
        (perm, savept)
    }

    #[test]
    fn names_and_thumbnails_come_from_the_wads() {
        let (perm, savept) = synthetic_wads();
        let art = art_from(&perm, &savept, &[4, 9]);
        assert_eq!(art.places.get(&4).map(String::as_str), Some("PLACE FOUR"));
        assert_eq!(art.places.get(&5).map(String::as_str), Some("PLACE FIVE"));
        assert_eq!(art.difficulties, vec!["Easy One".to_string(), "Normal One".to_string()]);
        assert_eq!(art.thumbnails.len(), 1, "save point 9 has no texture");
        let png = art.thumbnails[&4].strip_prefix("data:image/png;base64,").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(png).unwrap();
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!((image.width(), image.height()), (16, 4));
        // the pixel at x holds index x, whose colour is (x, 255 - x, 7) once the CSM1 order is undone; 0x80 is opaque
        for x in 0..16u32 {
            assert_eq!(image.get_pixel(x, 2).0, [x as u8, 255 - x as u8, 7, 255]);
        }
    }

    #[test]
    fn the_toc_names_part_size_and_sector() {
        let mut toc = Vec::new();
        for (name, part, size, sector) in
            [("R_PERM.WAD", 0u32, 100u32, 0u32), ("R_SAVEPT.WAD", 1, 64, 9), ("R_SAVEPT.WAD", 0, 64, 3)]
        {
            let mut field = [0u8; 12];
            field[..name.len()].copy_from_slice(name.as_bytes());
            toc.extend_from_slice(&field);
            for word in [part, size, sector] {
                toc.extend_from_slice(&word.to_le_bytes());
            }
        }
        toc.extend_from_slice(&[0u8; 12]); // the retail TOC ends with half an entry
        assert_eq!(toc_find(&toc, "R_SAVEPT.WAD"), Some(TocEntry { part: 1, size: 64, sector: 9 }));
        assert_eq!(toc_find(&toc, "r_perm.wad").map(|e| e.size), Some(100));
        assert_eq!(toc_find(&toc, "NONE.WAD"), None);
    }

    #[test]
    fn an_extracted_disc_is_read_through_its_toc() {
        let directory = tempfile::tempdir().unwrap();
        let (perm, savept) = synthetic_wads();
        // PART1.PAK: R_PERM.WAD at sector 0, R_SAVEPT.WAD at sector 2 (perm is shorter than two sectors)
        assert!(perm.len() <= 2 * SECTOR as usize);
        let mut pak = perm.clone();
        pak.resize(2 * SECTOR as usize, 0);
        pak.extend_from_slice(&savept);
        std::fs::write(directory.path().join("PART1.PAK"), &pak).unwrap();
        let mut toc = Vec::new();
        for (name, size, sector) in [("R_PERM.WAD", perm.len() as u32, 0u32), ("R_SAVEPT.WAD", savept.len() as u32, 2)]
        {
            let mut field = [0u8; 12];
            field[..name.len()].copy_from_slice(name.as_bytes());
            toc.extend_from_slice(&field);
            for word in [0u32, size, sector] {
                toc.extend_from_slice(&word.to_le_bytes());
            }
        }
        std::fs::write(directory.path().join("GODOFWAR.TOC"), &toc).unwrap();
        let elf = directory.path().join("SCUS_973.99");
        std::fs::write(&elf, b"x").unwrap();
        let art = read_save_art(&elf.to_string_lossy(), "", &[4]);
        assert_eq!(art.places.get(&4).map(String::as_str), Some("PLACE FOUR"));
        assert!(art.thumbnails.contains_key(&4));
        // no disc: nothing, and no error
        assert_eq!(read_save_art("", "", &[4]), SaveArt::default());
    }

    /// `GOW_TEST_GAME_ELF=<SCUS_973.99 of an installation> cargo test -p gow-core real_disc -- --nocapture` reads the real
    /// disc and prints what the page would show. Without the variable it does nothing.
    #[test]
    fn real_disc_names_its_save_points() {
        let Ok(elf) = std::env::var("GOW_TEST_GAME_ELF") else { return };
        let art = read_save_art(&elf, "", &[1, 4]);
        eprintln!(
            "{} places, difficulties {:?}, thumbnails {:?}",
            art.places.len(),
            art.difficulties,
            art.thumbnails.keys()
        );
        assert!(!art.places.is_empty() && art.difficulties.len() == 4 && art.thumbnails.len() == 2);
    }
}
