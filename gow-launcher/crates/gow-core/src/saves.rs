//! The player's saves: what the memory card holds, the dated copies the game makes at every altar (`save-backups/`, written by
//! the runtime, `src/lib/gow_save_backup.cpp`) and the actions the Saves page offers on them.
//!
//! The card is the PCSX2 `.ps2` image the runtime uses (512-byte pages, each followed by 16 spare bytes). Only what the page
//! shows is read, never written: the superblock, the FAT and the game's folder `BASCUS-97399GodOfWar`, whose file of the same
//! name is the game's own 68-byte slot index and whose `dataN.bin` files hold the saves (80 KiB each).
//!
//! Index layout (checked against real cards and against the game's own save code, 0x230634..0x230670): four records of 16
//! bytes and a trailing word. A record is `[u32 payload number, u32 0xCAFEBAD1, f32 play time in seconds, u16 save point,
//! u8 difficulty, u8 0]`; `0xFFFFFFFF` in the first word marks an empty slot. A payload that no record refers to is the
//! spare the game writes the next save to. The save point names the altar (and its thumbnail, `save_art.rs`); the
//! difficulty is 0 to 3, easiest first.
//!
//! Loading a slot while the game runs: [`request_load`] writes `<card folder>/load-request`, which the runtime takes on
//! its EE thread (`src/lib/gow_save_backup.cpp`) and walks the game's own path to that slot (`src/game/gow_save_load.cpp`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const PAGE: usize = 512;
const RAW_PAGE: usize = 528;
const CARD_IMAGE_SIZE: usize = RAW_PAGE * 16384;
const SAVE_FOLDER: &str = "BASCUS-97399GodOfWar";
const INDEX_MAGIC: u32 = 0xCAFE_BAD1;
const SLOTS: usize = 4;

/// One save of the card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSlot {
    /// Position in the game's index (0..=3), the order the game lists them.
    pub index: usize,
    /// The payload file (`data3.bin`).
    pub file: String,
    /// When the game wrote it, as seconds since 1970 (UTC). The card stores Japan time; this is already converted.
    pub modified_unix: i64,
    /// The zone the save stands in (`Athn02C`), the first name of the payload.
    pub zone: String,
    /// Play time in seconds, as the game shows it (`h:mm:ss`).
    pub play_seconds: f64,
    /// The altar the save was made at (1..=41 on the retail disc; 0 when unknown).
    pub save_point: u16,
    /// 0 to 3, easiest first.
    pub difficulty: u8,
}

/// A dated copy of the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBackup {
    /// The folder name (`20261112-103122`), which is also its identity.
    pub id: String,
    pub created: String,
    pub created_unix: i64,
    /// `auto` (the game completed a save), `manual` or `before-restore`.
    pub source: String,
    pub size: u64,
}

#[derive(Debug)]
pub enum SaveError {
    GameRunning,
    NoCard,
    NoBackup,
    Io(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GameRunning => write!(f, "the game is running"),
            Self::NoCard => write!(f, "there is no memory card"),
            Self::NoBackup => write!(f, "the backup is not there"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SaveError {}

fn io(error: impl std::fmt::Display) -> SaveError {
    SaveError::Io(error.to_string())
}

// ---------------------------------------------------------------------------------------------------------------------
// The card

struct Card {
    pages: Vec<u8>,
    pages_per_cluster: usize,
    alloc_offset: usize,
    root: usize,
    fat: Vec<u32>,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    bytes.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// One directory entry (512 bytes).
struct Entry {
    mode: u16,
    length: u32,
    cluster: usize,
    modified: [u8; 8],
    name: String,
}

impl Card {
    fn parse(image: &[u8]) -> Option<Self> {
        if image.len() != CARD_IMAGE_SIZE {
            return None;
        }
        let mut pages = Vec::with_capacity(image.len() / RAW_PAGE * PAGE);
        for page in 0..image.len() / RAW_PAGE {
            pages.extend_from_slice(&image[page * RAW_PAGE..page * RAW_PAGE + PAGE]);
        }
        let superblock = pages[..PAGE].to_vec();
        if !superblock.starts_with(b"Sony PS2 Memory Card Format") {
            return None;
        }
        let pages_per_cluster = u16_at(&superblock, 0x2A) as usize;
        let alloc_offset = u32_at(&superblock, 0x34) as usize;
        let root = u32_at(&superblock, 0x3C) as usize;
        if pages_per_cluster == 0 || pages_per_cluster > 8 {
            return None;
        }
        let mut card = Self { pages, pages_per_cluster, alloc_offset, root, fat: Vec::new() };
        let per_cluster = card.cluster_size() / 4;
        let mut fat = Vec::new();
        for index in 0..32 {
            let indirect = u32_at(&superblock, 0x50 + 4 * index);
            if indirect == 0 || indirect == u32::MAX {
                continue;
            }
            let indirect_data = card.cluster(indirect as usize)?.to_vec();
            for slot in 0..per_cluster {
                let fat_cluster = u32_at(&indirect_data, 4 * slot);
                if fat_cluster == u32::MAX {
                    continue;
                }
                let data = card.cluster(fat_cluster as usize)?;
                fat.extend((0..per_cluster).map(|i| u32_at(data, 4 * i)));
            }
        }
        card.fat = fat;
        Some(card)
    }

    fn cluster_size(&self) -> usize {
        self.pages_per_cluster * PAGE
    }

    /// An absolute cluster (superblock, FAT...).
    fn cluster(&self, number: usize) -> Option<&[u8]> {
        let start = number.checked_mul(self.cluster_size())?;
        self.pages.get(start..start + self.cluster_size())
    }

    /// The bytes of a file or directory that starts at `start` (a cluster relative to the allocatable area).
    fn chain(&self, start: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cluster = start;
        for _ in 0..self.fat.len() + 1 {
            let Some(data) = self.cluster(self.alloc_offset + cluster) else { break };
            out.extend_from_slice(data);
            let entry = self.fat.get(cluster).copied().unwrap_or(0);
            let next = (entry & 0x7FFF_FFFF) as usize;
            if entry & 0x8000_0000 == 0 || next == 0x7FFF_FFFF {
                break;
            }
            cluster = next;
        }
        out
    }

    fn entries(&self, start: usize, count: usize) -> Vec<Entry> {
        let raw = self.chain(start);
        (0..count)
            .filter_map(|i| raw.get(i * 512..(i + 1) * 512))
            .map(|e| {
                let name_bytes = &e[0x40..0x60];
                let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
                Entry {
                    mode: u16_at(e, 0),
                    length: u32_at(e, 4),
                    cluster: u32_at(e, 0x10) as usize,
                    modified: e[0x18..0x20].try_into().unwrap_or([0; 8]),
                    name: String::from_utf8_lossy(&name_bytes[..end]).into_owned(),
                }
            })
            .collect()
    }

    /// The entries of the root directory: its own `.` entry says how many there are.
    fn root_directory(&self) -> Vec<Entry> {
        let count = self.entries(self.root, 1).first().map_or(0, |e| e.length as usize).min(4096);
        self.entries(self.root, count)
    }

    /// The entries of a sub-directory: the count is in the entry of its parent (its own `.` entry says 0 on real cards).
    fn directory(&self, entry: &Entry) -> Vec<Entry> {
        self.entries(entry.cluster, (entry.length as usize).min(4096))
    }

    fn file(&self, entry: &Entry) -> Vec<u8> {
        let mut data = self.chain(entry.cluster);
        data.truncate(entry.length as usize);
        data
    }
}

const IS_DIRECTORY: u16 = 0x20;

/// Seconds since 1970 (UTC) of a card timestamp (`[_, sec, min, hour, day, month, year u16]`, Japan time).
fn card_time_unix(modified: [u8; 8]) -> i64 {
    let (sec, min, hour, day, month) =
        (modified[1] as i64, modified[2] as i64, modified[3] as i64, modified[4] as i64, modified[5] as i64);
    let year = u16::from_le_bytes([modified[6], modified[7]]) as i64;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return 0;
    }
    // days from civil (Howard Hinnant)
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hour * 3600 + min * 60 + sec - 9 * 3600
}

/// The saves of a card image, in the game's order. Empty when the card has none or is not readable.
pub fn read_card_slots(image: &[u8]) -> Vec<CardSlot> {
    let Some(card) = Card::parse(image) else { return Vec::new() };
    let root = card.root_directory();
    let Some(folder) = root.iter().find(|e| e.mode & IS_DIRECTORY != 0 && e.name == SAVE_FOLDER) else {
        return Vec::new();
    };
    let files = card.directory(folder);
    let Some(index_entry) = files.iter().find(|e| e.mode & IS_DIRECTORY == 0 && e.name == SAVE_FOLDER) else {
        return Vec::new();
    };
    let index = card.file(index_entry);
    let mut slots = Vec::new();
    for slot in 0..SLOTS {
        let at = slot * 16;
        let (payload, magic) = (u32_at(&index, at), u32_at(&index, at + 4));
        if payload == u32::MAX || magic != INDEX_MAGIC {
            continue;
        }
        let name = format!("data{payload}.bin");
        let Some(entry) = files.iter().find(|e| e.mode & IS_DIRECTORY == 0 && e.name == name) else { continue };
        let data = card.file(entry);
        let zone_bytes = data.get(8..8 + 16).unwrap_or(&[]);
        let end = zone_bytes.iter().position(|&b| b == 0).unwrap_or(zone_bytes.len());
        let play = f32::from_bits(u32_at(&index, at + 8));
        slots.push(CardSlot {
            index: slot,
            file: name,
            modified_unix: card_time_unix(entry.modified),
            zone: String::from_utf8_lossy(&zone_bytes[..end]).into_owned(),
            play_seconds: if play.is_finite() && play >= 0.0 { f64::from(play) } else { 0.0 },
            save_point: u16_at(&index, at + 12),
            difficulty: index.get(at + 14).copied().unwrap_or(0),
        });
    }
    slots
}

/// The saves of the card file at `path`.
pub fn read_card_file_slots(path: &Path) -> Vec<CardSlot> {
    fs::read(path).map(|image| read_card_slots(&image)).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------------
// Loading a slot while the game runs

/// `<card folder>/load-request`, the file the runtime looks for (`gow_save_backup.h`).
pub fn load_request_file(card: &Path) -> PathBuf {
    card.parent().unwrap_or_else(|| Path::new("")).join("load-request")
}

/// Asks the game for slot `index` (0..=3): `slot=<index>` and `created_unix=<now>`, published with a rename so the
/// runtime never reads half a file. The runtime ignores a request older than ten minutes, so one written before the game
/// starts is taken once its main menu is up, and a leftover is not.
pub fn request_load(card: &Path, index: usize, now_unix: i64) -> Result<(), SaveError> {
    if index >= SLOTS {
        return Err(SaveError::Io(format!("there is no slot {index}")));
    }
    if card.as_os_str().is_empty() {
        return Err(SaveError::NoCard);
    }
    let target = load_request_file(card);
    let temporary = target.with_extension("tmp");
    fs::write(&temporary, format!("slot={index}\ncreated_unix={now_unix}\n")).map_err(io)?;
    fs::rename(&temporary, &target).map_err(io)
}

// ---------------------------------------------------------------------------------------------------------------------
// The history

/// `<card folder>/save-backups`, where the runtime writes its copies.
pub fn backups_folder(card: &Path) -> PathBuf {
    card.parent().unwrap_or_else(|| Path::new("")).join("save-backups")
}

fn is_backup_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 15
        && bytes.iter().take(15).enumerate().all(|(i, b)| if i == 8 { *b == b'-' } else { b.is_ascii_digit() })
        && (bytes.len() == 15 || bytes[15] == b'-')
}

/// The copies of the card, newest first. A folder without its card file (a copy that was cut short) is not listed.
pub fn list_backups(card: &Path) -> Vec<SaveBackup> {
    #[derive(Deserialize)]
    struct Meta {
        created: String,
        created_unix: i64,
        source: String,
        size: u64,
    }
    let Ok(read) = fs::read_dir(backups_folder(card)) else { return Vec::new() };
    let mut backups: Vec<SaveBackup> = read
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            if !is_backup_name(&id) || !entry.path().join("Mcd001.ps2").is_file() {
                return None;
            }
            let meta: Meta = serde_json::from_str(&fs::read_to_string(entry.path().join("backup.json")).ok()?).ok()?;
            Some(SaveBackup {
                id,
                created: meta.created,
                created_unix: meta.created_unix,
                source: meta.source,
                size: meta.size,
            })
        })
        .collect();
    backups.sort_by(|a, b| b.id.cmp(&a.id));
    backups
}

/// The saves a backup holds (the Saves page shows them next to the restore button).
pub fn backup_slots(card: &Path, id: &str) -> Vec<CardSlot> {
    if !is_backup_name(id) {
        return Vec::new();
    }
    read_card_file_slots(&backups_folder(card).join(id).join("Mcd001.ps2"))
}

fn local_stamp(unix: i64) -> String {
    // the folder name only has to be unique and sort by time: UTC is enough for the "before restore" copies
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Copies the card to a new dated folder. `source` is stored in `backup.json` (`manual`, `before-restore`).
pub fn make_backup(card: &Path, source: &str, now_unix: i64) -> Result<SaveBackup, SaveError> {
    let size = fs::metadata(card).map_err(|_| SaveError::NoCard)?.len();
    if size != CARD_IMAGE_SIZE as u64 {
        return Err(SaveError::Io("the memory card file has an unexpected size".into()));
    }
    let root = backups_folder(card);
    fs::create_dir_all(&root).map_err(io)?;
    let stamp = local_stamp(now_unix);
    let mut folder = None;
    for attempt in 1..100 {
        let name = if attempt == 1 { stamp.clone() } else { format!("{stamp}-{attempt}") };
        match fs::create_dir(root.join(&name)) {
            Ok(()) => {
                folder = Some(name);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io(error)),
        }
    }
    let id = folder.ok_or_else(|| SaveError::Io("no free backup folder".into()))?;
    let directory = root.join(&id);
    let result = (|| -> Result<(), SaveError> {
        let temporary = directory.join("Mcd001.ps2.tmp");
        fs::copy(card, &temporary).map_err(io)?;
        let created = format!(
            "{}-{}-{}T{}:{}:{}",
            &stamp[0..4],
            &stamp[4..6],
            &stamp[6..8],
            &stamp[9..11],
            &stamp[11..13],
            &stamp[13..15]
        );
        let meta = format!(
            "{{\n  \"created\": \"{created}\",\n  \"created_unix\": {now_unix},\n  \"source\": \"{source}\",\n  \"size\": {size},\n  \"card\": \"Mcd001.ps2\"\n}}\n"
        );
        fs::write(directory.join("backup.json"), meta).map_err(io)?;
        fs::rename(&temporary, directory.join("Mcd001.ps2")).map_err(io)
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&directory); // only the folder this call reserved
        return Err(error);
    }
    Ok(SaveBackup { id, created: String::new(), created_unix: now_unix, source: source.into(), size })
}

/// Puts a backup's card in place of the active one. The active card is copied to the history first (`before-restore`), so a
/// restore can itself be undone. Only with the game closed: it keeps the card image in memory and would write it back.
pub fn restore_backup(card: &Path, id: &str, game_running: bool, now_unix: i64) -> Result<(), SaveError> {
    if game_running {
        return Err(SaveError::GameRunning);
    }
    if !is_backup_name(id) {
        return Err(SaveError::NoBackup);
    }
    let source = backups_folder(card).join(id).join("Mcd001.ps2");
    if fs::metadata(&source).map(|m| m.len()).ok() != Some(CARD_IMAGE_SIZE as u64) {
        return Err(SaveError::NoBackup);
    }
    if card.is_file() {
        make_backup(card, "before-restore", now_unix)?;
    }
    let temporary = card.with_extension("ps2.restoring");
    fs::copy(&source, &temporary).map_err(io)?;
    // the runtime's own safety copy of the previous session no longer describes this card
    let _ = fs::remove_file(card.with_extension("ps2.bak"));
    let mut file = fs::OpenOptions::new().write(true).open(&temporary).map_err(io)?;
    file.flush().map_err(io)?;
    drop(file);
    fs::rename(&temporary, card).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card with the superblock, one FAT cluster and a root directory holding the game's folder, its index and one payload.
    fn synthetic_card(payload_zone: &str, index: [u32; 5]) -> Vec<u8> {
        const PPC: usize = 2;
        let mut pages = vec![0xFFu8; 16384 * PAGE];
        let set = |pages: &mut Vec<u8>, at: usize, bytes: &[u8]| pages[at..at + bytes.len()].copy_from_slice(bytes);
        let mut superblock = vec![0u8; PAGE];
        superblock[..28].copy_from_slice(b"Sony PS2 Memory Card Format ");
        superblock[0x2A..0x2C].copy_from_slice(&(PPC as u16).to_le_bytes());
        superblock[0x34..0x38].copy_from_slice(&41u32.to_le_bytes()); // allocatable area starts at cluster 41
        superblock[0x3C..0x40].copy_from_slice(&0u32.to_le_bytes());
        superblock[0x50..0x54].copy_from_slice(&8u32.to_le_bytes()); // indirect FAT cluster
        for i in 1..32 {
            superblock[0x50 + 4 * i..0x54 + 4 * i].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        set(&mut pages, 0, &superblock);
        // indirect cluster 8: first FAT cluster is 9
        let mut indirect = vec![0xFFu8; PPC * PAGE];
        indirect[..4].copy_from_slice(&9u32.to_le_bytes());
        set(&mut pages, 8 * PPC * PAGE, &indirect);
        // FAT (cluster 9): clusters of the allocatable area. A directory entry is 512 bytes and a cluster holds two, so the
        // root (clusters 0 -> 1) and the game's folder (2 -> 3) are chains of two; the index (4) and the payload (5) are one.
        let mut fat = vec![0u8; PPC * PAGE];
        let end = 0x8000_0000u32 | 0x7FFF_FFFF;
        for (cluster, value) in [(0usize, 0x8000_0001u32), (1, end), (2, 0x8000_0003), (3, end), (4, end), (5, end)] {
            fat[cluster * 4..cluster * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        set(&mut pages, 9 * PPC * PAGE, &fat);
        let entry = |mode: u16, length: u32, cluster: u32, name: &str| {
            let mut e = vec![0u8; 512];
            e[0..2].copy_from_slice(&mode.to_le_bytes());
            e[4..8].copy_from_slice(&length.to_le_bytes());
            e[0x10..0x14].copy_from_slice(&cluster.to_le_bytes());
            // 2026-11-06 12:48:37 Japan time
            e[0x18..0x20].copy_from_slice(&[0, 37, 48, 12, 6, 11, 0xEA, 0x07]);
            e[0x40..0x40 + name.len()].copy_from_slice(name.as_bytes());
            e
        };
        let base = 41 * PPC * PAGE;
        // root (cluster 0): ".", "..", the game's folder (cluster 2)
        let mut root = Vec::new();
        root.extend(entry(0x8427, 3, 0, "."));
        root.extend(entry(0x8427, 0, 0, ".."));
        root.extend(entry(0x8427, 4, 2, SAVE_FOLDER)); // four entries: ".", "..", the index, the payload
        set(&mut pages, base, &root);
        // folder (cluster 2): ".", "..", the index (cluster 4), data0.bin (cluster 5)
        let mut folder = Vec::new();
        folder.extend(entry(0x8427, 0, 0, ".")); // a real card says 0 here: the count is in the parent's entry
        folder.extend(entry(0x8427, 0, 0, ".."));
        folder.extend(entry(0x8497, 68, 4, SAVE_FOLDER));
        folder.extend(entry(0x8497, 64, 5, "data0.bin"));
        set(&mut pages, base + 2 * PPC * PAGE, &folder);
        let mut index_bytes = Vec::new();
        for word in index {
            index_bytes.extend_from_slice(&word.to_le_bytes());
        }
        index_bytes.resize(68, 0);
        set(&mut pages, base + 4 * PPC * PAGE, &index_bytes);
        let mut payload = vec![0u8; 64];
        payload[0..4].copy_from_slice(&INDEX_MAGIC.to_le_bytes());
        payload[8..8 + payload_zone.len()].copy_from_slice(payload_zone.as_bytes());
        set(&mut pages, base + 5 * PPC * PAGE, &payload);
        // the .ps2 layout: 16 spare bytes after every page
        let mut image = Vec::with_capacity(CARD_IMAGE_SIZE);
        for page in pages.chunks(PAGE) {
            image.extend_from_slice(page);
            image.extend_from_slice(&[0u8; 16]);
        }
        image
    }

    #[test]
    fn reads_the_slot_the_index_points_to() {
        let card = synthetic_card("Athn02C", [0, INDEX_MAGIC, 0x45356F55, 4, u32::MAX]);
        let slots = read_card_slots(&card);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].index, 0);
        assert_eq!(slots[0].file, "data0.bin");
        assert_eq!(slots[0].zone, "Athn02C");
        // the record's third word is the play time (2902.96 s, 0:48:22) and the fourth the save point and difficulty
        assert!((slots[0].play_seconds - 2902.958).abs() < 0.01);
        assert_eq!((slots[0].save_point, slots[0].difficulty), (4, 0));
        // 2026-11-06 12:48:37 in Japan is 03:48:37 UTC
        assert_eq!(slots[0].modified_unix, 1_793_936_917);
    }

    #[test]
    fn an_empty_index_or_a_stranger_card_lists_nothing() {
        assert!(read_card_slots(&synthetic_card("Athn02C", [u32::MAX; 5])).is_empty());
        assert!(read_card_slots(&vec![0u8; CARD_IMAGE_SIZE]).is_empty());
        assert!(read_card_slots(&[0u8; 100]).is_empty());
    }

    #[test]
    fn history_lists_complete_copies_newest_first_and_restores_with_a_safety_copy() {
        let root = std::env::temp_dir().join(format!("gow-saves-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let card = root.join("Mcd001.ps2");
        let first = synthetic_card("Athn02C", [0, INDEX_MAGIC, 1, 4, u32::MAX]);
        fs::write(&card, &first).unwrap();
        let one = make_backup(&card, "manual", 1_793_936_917).unwrap();
        let second = synthetic_card("Athn03A", [0, INDEX_MAGIC, 2, 4, u32::MAX]);
        fs::write(&card, &second).unwrap();
        let two = make_backup(&card, "manual", 1_793_936_999).unwrap();
        let listed = list_backups(&card);
        assert_eq!(listed.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), vec![two.id.as_str(), one.id.as_str()]);
        assert_eq!(backup_slots(&card, &one.id)[0].zone, "Athn02C");
        // an unfinished copy (no card file) and a stranger folder are not listed
        fs::create_dir_all(backups_folder(&card).join("20261106-120000")).unwrap();
        fs::create_dir_all(backups_folder(&card).join("not-a-backup")).unwrap();
        assert_eq!(list_backups(&card).len(), 2);
        // restore: refused while the game runs, otherwise the old card comes back and the current one is kept
        assert!(matches!(restore_backup(&card, &one.id, true, 1_793_937_100), Err(SaveError::GameRunning)));
        restore_backup(&card, &one.id, false, 1_793_937_100).unwrap();
        assert_eq!(fs::read(&card).unwrap(), first);
        let kept = list_backups(&card);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0].source, "before-restore");
        assert_eq!(backup_slots(&card, &kept[0].id)[0].zone, "Athn03A");
        assert!(matches!(restore_backup(&card, "../x", false, 0), Err(SaveError::NoBackup)));
        let _ = fs::remove_dir_all(&root);
    }

    /// `GOW_TEST_CARD=<card.ps2> cargo test -p gow-core real_card -- --nocapture` reads a real card (a PCSX2 card with a
    /// save, or one the port wrote) and prints what the page would show. Without the variable it does nothing.
    #[test]
    fn real_card_lists_its_saves() {
        let Ok(path) = std::env::var("GOW_TEST_CARD") else { return };
        let slots = read_card_file_slots(Path::new(&path));
        for slot in &slots {
            eprintln!(
                "slot {} {} zone {} modified {} play {:.0} s save point {} difficulty {}",
                slot.index,
                slot.file,
                slot.zone,
                slot.modified_unix,
                slot.play_seconds,
                slot.save_point,
                slot.difficulty
            );
        }
        assert!(!slots.is_empty(), "the card has no God of War save");
    }

    #[test]
    fn a_load_request_is_published_whole() {
        let root = tempfile::tempdir().unwrap();
        let card = root.path().join("Mcd001.ps2");
        request_load(&card, 3, 1_793_936_917).unwrap();
        let text = fs::read_to_string(load_request_file(&card)).unwrap();
        assert_eq!(text, "slot=3\ncreated_unix=1793936917\n");
        assert!(!root.path().join("load-request.tmp").exists());
        assert!(request_load(&card, 4, 0).is_err());
        assert!(matches!(request_load(Path::new(""), 0, 0), Err(SaveError::NoCard)));
    }

    #[test]
    fn card_time_is_japan_time() {
        assert_eq!(card_time_unix([0, 0, 0, 9, 1, 1, 0xDA, 0x07]), 1_262_304_000); // 2010-01-01 09:00 JST = 00:00 UTC
        assert_eq!(card_time_unix([0, 0, 0, 0, 0, 0, 0, 0]), 0);
    }
}
