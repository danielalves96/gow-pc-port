//! GOW-Port (0.3.1): reads a CHD disc image (MAME's "Compressed Hunks of Data", made by `chdman`) and writes the ISO
//! inside it, so a player with a `.chd` can install without converting it with another program.
//!
//! Only version 5 is read (every `chdman` since 2012 writes it), with the codecs `chdman` uses for discs: `cdlz`,
//! `cdzl`, `cdzs` and `cdfl` for images made with `createcd` (the PS2 CHDs found in the wild are mostly these, a DVD
//! stored as one MODE1 track), and `lzma`, `zlib`, `zstd`, `huff` and `flac` for `createdvd` / `createhd` images.
//! Every hunk is checked against the CRC-16 of its map entry, the whole raw data against the header's SHA-1, and the
//! metadata against the combined SHA-1, as `chdman verify` does. A CHD that needs a parent CHD is refused.
//!
//! The format, as MAME's `chd.cpp`, `chdcodec.cpp`, `huffman.cpp` and `cdrom.cpp` define it:
//! - a 124-byte header (big-endian): codecs, logical size, map and metadata offsets, hunk and unit sizes, SHA-1s;
//! - a map with one entry per hunk (codec, compressed length, file offset, CRC-16), itself Huffman-coded;
//! - metadata entries in a linked list; a CD keeps one `CHT2` text entry per track;
//! - a CD hunk holds whole 2,448-byte frames (2,352 bytes of sector and 96 of subcode), each track padded to a multiple of
//!   four frames; a cooked MODE1 track keeps its 2,048 bytes at the start of each sector.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};

use crate::flac;
use crate::hash::Sha1;

/// The message of a conversion the caller stopped.
pub const CANCELLED: &str = "cancelled";

const HEADER_V5_BYTES: usize = 124;
const CD_FRAME_BYTES: usize = 2448;
const CD_SECTOR_BYTES: usize = 2352;
const CD_SUBCODE_BYTES: usize = 96;
const CD_TRACK_PADDING: u64 = 4;
const CD_SYNC_HEADER: [u8; 12] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
/// Metadata entries whose content is part of the combined SHA-1.
const METADATA_CHECKSUM_FLAG: u8 = 0x01;

const fn tag(text: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*text)
}

const CODEC_ZLIB: u32 = tag(b"zlib");
const CODEC_LZMA: u32 = tag(b"lzma");
const CODEC_HUFF: u32 = tag(b"huff");
const CODEC_FLAC: u32 = tag(b"flac");
const CODEC_ZSTD: u32 = tag(b"zstd");
const CODEC_CD_ZLIB: u32 = tag(b"cdzl");
const CODEC_CD_LZMA: u32 = tag(b"cdlz");
const CODEC_CD_FLAC: u32 = tag(b"cdfl");
const CODEC_CD_ZSTD: u32 = tag(b"cdzs");

const META_CD_TRACK: u32 = tag(b"CHTR");
const META_CD_TRACK2: u32 = tag(b"CHT2");
const META_CD_OLD: u32 = tag(b"CHCD");
const META_GD_TRACK: u32 = tag(b"CHGD");

// Map entry types of a compressed v5 map.
const MAP_CODEC_0: u8 = 0;
const MAP_CODEC_3: u8 = 3;
const MAP_NONE: u8 = 4;
const MAP_SELF: u8 = 5;
const MAP_PARENT: u8 = 6;
const MAP_RLE_SMALL: u8 = 7;
const MAP_RLE_LARGE: u8 = 8;
const MAP_SELF_0: u8 = 9;
const MAP_SELF_1: u8 = 10;
const MAP_PARENT_SELF: u8 = 11;
const MAP_PARENT_0: u8 = 12;
const MAP_PARENT_1: u8 = 13;
/// An entry of an uncompressed CHD that points at no data: the hunk is zeros.
const MAP_ZERO: u8 = 0xFE;

fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn be48(bytes: &[u8]) -> u64 {
    bytes[..6].iter().fold(0, |value, &byte| (value << 8) | u64::from(byte))
}

fn be64(bytes: &[u8]) -> u64 {
    bytes[..8].iter().fold(0, |value, &byte| (value << 8) | u64::from(byte))
}

/// CRC-16/CCITT (polynomial 0x1021, starting at 0xFFFF), MAME's `crc16_creator`.
pub fn crc16(data: &[u8]) -> u16 {
    static TABLE: std::sync::OnceLock<[u16; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u16; 256];
        for (index, entry) in table.iter_mut().enumerate() {
            let mut value = (index as u16) << 8;
            for _ in 0..8 {
                value = if value & 0x8000 != 0 { (value << 1) ^ 0x1021 } else { value << 1 };
            }
            *entry = value;
        }
        table
    });
    data.iter().fold(0xFFFF, |crc, &byte| (crc << 8) ^ table[usize::from((crc >> 8) as u8 ^ byte)])
}

fn codec_name(codec: u32) -> String {
    if codec == 0 {
        return "none".into();
    }
    let bytes = codec.to_be_bytes();
    if bytes.iter().all(u8::is_ascii_graphic) {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!("{codec:#010x}")
    }
}

fn damaged(what: impl std::fmt::Display) -> String {
    format!("the CHD is damaged: {what}")
}

// ---------------------------------------------------------------------------------------------------------------------
// MAME's bit reader and Huffman decoder (huffman.cpp), used by the map and by the `huff` codec.

/// Reads bits most significant first; reading past the end gives zeros and is caught by [`BitReader::overflow`].
struct BitReader<'a> {
    data: &'a [u8],
    buffer: u32,
    bits: i32,
    offset: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, buffer: 0, bits: 0, offset: 0 }
    }

    fn peek(&mut self, count: i32) -> u32 {
        if count == 0 {
            return 0;
        }
        if count > self.bits {
            while self.bits <= 24 {
                if let Some(&byte) = self.data.get(self.offset) {
                    self.buffer |= u32::from(byte) << (24 - self.bits);
                }
                self.offset += 1;
                self.bits += 8;
            }
        }
        self.buffer >> (32 - count)
    }

    fn remove(&mut self, count: i32) {
        self.buffer = self.buffer.checked_shl(count as u32).unwrap_or(0);
        self.bits -= count;
    }

    fn read(&mut self, count: i32) -> u32 {
        let value = self.peek(count);
        self.remove(count);
        value
    }

    fn overflow(&self) -> bool {
        self.offset.saturating_sub((self.bits / 8) as usize) > self.data.len()
    }
}

/// A canonical Huffman decoder with MAME's code assignment (longer codes get the lower numbers).
struct Huffman {
    max_bits: i32,
    lengths: Vec<u8>,
    lookup: Vec<u32>,
}

impl Huffman {
    fn new(codes: usize, max_bits: i32) -> Self {
        Self { max_bits, lengths: vec![0; codes], lookup: vec![0; 1 << max_bits] }
    }

    /// Gives every code its bits from the lengths and fills the lookup table.
    fn build(&mut self) -> Result<(), String> {
        let mut histogram = [0u32; 33];
        for &length in &self.lengths {
            if i32::from(length) > self.max_bits {
                return Err(damaged("a Huffman code is too long"));
            }
            histogram[usize::from(length)] += 1;
        }
        let mut start = 0u32;
        for length in (1..=32).rev() {
            let next = (start + histogram[length]) >> 1;
            if length != 1 && next * 2 != start + histogram[length] {
                return Err(damaged("a Huffman tree is inconsistent"));
            }
            histogram[length] = start;
            start = next;
        }
        self.lookup.fill(0);
        for (symbol, &length) in self.lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let code = histogram[usize::from(length)];
            histogram[usize::from(length)] += 1;
            let shift = self.max_bits - i32::from(length);
            let first = (code as usize) << shift;
            let last = ((code as usize + 1) << shift) - 1;
            if last >= self.lookup.len() {
                return Err(damaged("a Huffman code does not fit its table"));
            }
            let value = ((symbol as u32) << 5) | u32::from(length & 0x1F);
            self.lookup[first..=last].fill(value);
        }
        Ok(())
    }

    fn decode(&self, bits: &mut BitReader) -> u32 {
        let entry = self.lookup[bits.peek(self.max_bits) as usize];
        bits.remove((entry & 0x1F) as i32);
        entry >> 5
    }

    /// The tree as the v5 map stores it: code lengths with a run-length escape.
    fn import_rle(&mut self, bits: &mut BitReader) -> Result<(), String> {
        let field = if self.max_bits >= 16 {
            5
        } else if self.max_bits >= 8 {
            4
        } else {
            3
        };
        let codes = self.lengths.len();
        let mut node = 0;
        while node < codes {
            let length = bits.read(field);
            if length != 1 {
                self.lengths[node] = length as u8;
                node += 1;
                continue;
            }
            let length = bits.read(field);
            if length == 1 {
                self.lengths[node] = 1;
                node += 1;
            } else {
                let repeat = bits.read(field) as usize + 3;
                if node + repeat > codes {
                    return Err(damaged("the map's Huffman tree overflows"));
                }
                self.lengths[node..node + repeat].fill(length as u8);
                node += repeat;
            }
        }
        self.build()?;
        if bits.overflow() {
            return Err(damaged("the map's Huffman tree is cut"));
        }
        Ok(())
    }

    /// The tree as the `huff` codec stores it: the lengths coded with a small Huffman tree of their own.
    fn import_huffman(&mut self, bits: &mut BitReader) -> Result<(), String> {
        let mut small = Huffman::new(24, 6);
        small.lengths[0] = bits.read(3) as u8;
        let start = bits.read(3) as usize + 1;
        let mut count = 0;
        for (index, length) in small.lengths.iter_mut().enumerate().skip(1) {
            if index < start || count == 7 {
                *length = 0;
            } else {
                count = bits.read(3);
                *length = if count == 7 { 0 } else { count as u8 };
            }
        }
        small.build()?;
        let codes = self.lengths.len();
        let mut rle_bits = 0;
        let mut rest = codes - 9;
        while rest != 0 {
            rest >>= 1;
            rle_bits += 1;
        }
        let (mut last, mut node) = (0u8, 0usize);
        while node < codes {
            let value = small.decode(bits);
            if value != 0 {
                last = (value - 1) as u8;
                self.lengths[node] = last;
                node += 1;
            } else {
                let mut repeat = bits.read(3) as usize + 2;
                if repeat == 9 {
                    repeat += bits.read(rle_bits) as usize;
                }
                while repeat != 0 && node < codes {
                    self.lengths[node] = last;
                    node += 1;
                    repeat -= 1;
                }
            }
        }
        self.build()?;
        if bits.overflow() {
            return Err(damaged("a Huffman tree is cut"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The CD sector ECC (P and Q parity of a MODE1 sector), which the CD codecs drop when it can be regenerated.

fn ecc_tables() -> &'static ([u8; 256], [u8; 256]) {
    static TABLES: std::sync::OnceLock<([u8; 256], [u8; 256])> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let (mut forward, mut backward) = ([0u8; 256], [0u8; 256]);
        for i in 0..256usize {
            let doubled = (i << 1) ^ if i & 0x80 != 0 { 0x11D } else { 0 };
            forward[i] = doubled as u8;
            backward[i ^ doubled] = i as u8;
        }
        (forward, backward)
    })
}

fn ecc_block(
    sector: &mut [u8],
    major_count: usize,
    minor_count: usize,
    major_mult: usize,
    minor_inc: usize,
    dest: usize,
) {
    let (forward, backward) = ecc_tables();
    let size = major_count * minor_count;
    for major in 0..major_count {
        let mut index = (major >> 1) * major_mult + (major & 1);
        let (mut a, mut b) = (0u8, 0u8);
        for _ in 0..minor_count {
            let value = sector[0x0C + index];
            index += minor_inc;
            if index >= size {
                index -= size;
            }
            a ^= value;
            b ^= value;
            a = forward[usize::from(a)];
        }
        a = backward[usize::from(forward[usize::from(a)] ^ b)];
        sector[dest + major] = a;
        sector[dest + major + major_count] = a ^ b;
    }
}

/// Writes the P and Q parity of a MODE1 sector (MAME's `ecc_generate`).
fn ecc_generate(sector: &mut [u8]) {
    ecc_block(sector, 86, 24, 2, 86, 0x81C);
    ecc_block(sector, 52, 43, 86, 88, 0x8C8);
}

// ---------------------------------------------------------------------------------------------------------------------
// The header, the map and the metadata.

#[derive(Debug, Clone, Copy)]
struct MapEntry {
    kind: u8,
    length: u32,
    offset: u64,
    crc: u16,
}

/// What a data track of a CD image is: where the 2,048 bytes of each sector are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub number: u32,
    /// `MODE1`, `MODE1_RAW`, `MODE2_RAW`, `AUDIO`...
    pub kind: String,
    /// Frames of the track in the CHD, its stored pregap included.
    pub frames: u64,
    /// First frame of the track in the CHD.
    pub first_frame: u64,
    /// Stored pregap frames to skip before the data.
    pub pregap: u64,
}

impl Track {
    /// Where the user data of a sector starts inside its 2,352 bytes, for a track an ISO can be made of.
    fn data_offset(&self) -> Option<usize> {
        match self.kind.as_str() {
            "MODE1" | "MODE1/2048" | "MODE2_FORM1" | "MODE2/2048" => Some(0),
            "MODE1_RAW" | "MODE1/2352" => Some(16),
            "MODE2" | "MODE2/2336" | "MODE2_FORM_MIX" => Some(8),
            "MODE2_RAW" | "MODE2/2352" | "CDI/2352" => Some(24),
            _ => None,
        }
    }
}

/// How the data inside the CHD becomes an ISO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// A CD image (`createcd`): the ISO is the user data of the first data track.
    Cd { tracks: Vec<Track> },
    /// A DVD or a plain image (`createdvd`, `createhd`): the ISO is the raw data itself.
    Raw,
}

/// What the header and the metadata say, read without decoding any hunk.
#[derive(Debug, Clone)]
pub struct Info {
    pub version: u32,
    pub codecs: Vec<String>,
    pub logical_bytes: u64,
    pub hunk_bytes: u32,
    pub unit_bytes: u32,
    pub hunks: u32,
    pub layout: Layout,
    /// Size of the ISO this CHD gives.
    pub iso_bytes: u64,
    pub raw_sha1: [u8; 20],
    pub sha1: [u8; 20],
}

impl Info {
    pub fn kind(&self) -> &'static str {
        match self.layout {
            Layout::Cd { .. } => "CD",
            Layout::Raw => "DVD",
        }
    }
}

/// An opened CHD: its header, map and metadata.
struct Chd {
    path: PathBuf,
    info: Info,
    codecs: [u32; 4],
    map: Vec<MapEntry>,
    /// For the combined SHA-1: tag and SHA-1 of every checksummed metadata entry.
    metadata_hashes: Vec<[u8; 24]>,
    /// The track an ISO is made of, for a CD: first frame, frames, data offset.
    iso_track: Option<(u64, u64, usize)>,
}

fn read_exact_at(file: &mut File, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
    file.seek(SeekFrom::Start(offset)).and_then(|_| file.read_exact(buffer)).map_err(|_| damaged("it ends too soon"))
}

fn parse_track(text: &str) -> Result<(u32, String, u64, u64, bool), String> {
    let field = |name: &str| {
        text.split_whitespace()
            .find_map(|part| part.strip_prefix(name).and_then(|rest| rest.strip_prefix(':')))
            .map(str::to_owned)
    };
    let number = |name: &str| field(name).and_then(|value| value.parse::<u64>().ok());
    let track = number("TRACK").ok_or_else(|| damaged(format!("a track entry is unreadable: {text}")))?;
    let kind = field("TYPE").ok_or_else(|| damaged(format!("a track entry has no type: {text}")))?;
    let frames = number("FRAMES").ok_or_else(|| damaged(format!("a track entry has no frame count: {text}")))?;
    let pregap = number("PREGAP").unwrap_or(0);
    // "V" pregap types mean the pregap sectors are stored in the CHD.
    let stored_pregap = field("PGTYPE").is_some_and(|value| value.starts_with('V'));
    Ok((track as u32, kind, frames, pregap, stored_pregap))
}

/// The metadata entries (tag and data) and, for the combined SHA-1, the tag and SHA-1 of each checksummed one.
type Metadata = (Vec<(u32, Vec<u8>)>, Vec<[u8; 24]>);

fn read_metadata(file: &mut File, mut offset: u64) -> Result<Metadata, String> {
    let (mut entries, mut hashes) = (Vec::new(), Vec::new());
    let mut seen = 0;
    while offset != 0 {
        seen += 1;
        if seen > 4096 {
            return Err(damaged("its metadata loops"));
        }
        let mut head = [0u8; 16];
        read_exact_at(file, offset, &mut head)?;
        let kind = be32(&head);
        let flags = head[4];
        let length = (be32(&head[4..]) & 0x00FF_FFFF) as usize;
        let mut data = vec![0u8; length];
        read_exact_at(file, offset + 16, &mut data)?;
        if flags & METADATA_CHECKSUM_FLAG != 0 {
            let mut entry = [0u8; 24];
            entry[..4].copy_from_slice(&kind.to_be_bytes());
            entry[4..].copy_from_slice(&Sha1::digest(&data));
            hashes.push(entry);
        }
        entries.push((kind, data));
        offset = be64(&head[8..]);
    }
    Ok((entries, hashes))
}

impl Chd {
    fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut header = [0u8; HEADER_V5_BYTES];
        file.read_exact(&mut header[..16]).map_err(|_| "not a CHD file (too short)".to_owned())?;
        if &header[..8] != b"MComprHD" {
            return Err("not a CHD file".into());
        }
        let version = be32(&header[12..]);
        if version != 5 {
            return Err(format!(
                "CHD version {version} is not supported (only version 5; convert it with `chdman copy` first)"
            ));
        }
        file.read_exact(&mut header[16..]).map_err(|_| damaged("its header is cut"))?;
        let codecs = [be32(&header[16..]), be32(&header[20..]), be32(&header[24..]), be32(&header[28..])];
        let logical_bytes = be64(&header[32..]);
        let map_offset = be64(&header[40..]);
        let meta_offset = be64(&header[48..]);
        let hunk_bytes = be32(&header[56..]);
        let unit_bytes = be32(&header[60..]);
        let mut raw_sha1 = [0u8; 20];
        raw_sha1.copy_from_slice(&header[64..84]);
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(&header[84..104]);
        if header[104..124].iter().any(|&byte| byte != 0) {
            return Err("this CHD needs a parent CHD (a delta image); use a complete CHD".into());
        }
        if hunk_bytes == 0 || unit_bytes == 0 || hunk_bytes % unit_bytes != 0 || hunk_bytes > 16 << 20 {
            return Err(damaged("its hunk size is invalid"));
        }
        let hunks = logical_bytes.div_ceil(u64::from(hunk_bytes));
        if hunks > u64::from(u32::MAX) {
            return Err(damaged("it has too many hunks"));
        }
        let hunks = hunks as u32;

        let map = if codecs[0] == 0 {
            Self::read_raw_map(&mut file, map_offset, hunks, hunk_bytes)?
        } else {
            Self::read_compressed_map(&mut file, map_offset, hunks, hunk_bytes, unit_bytes)?
        };

        let (metadata, metadata_hashes) = read_metadata(&mut file, meta_offset)?;
        let mut tracks = Vec::new();
        let mut is_cd = false;
        let mut next_frame = 0u64;
        for (kind, data) in &metadata {
            match *kind {
                META_CD_TRACK | META_CD_TRACK2 => {
                    is_cd = true;
                    let text = String::from_utf8_lossy(data);
                    let text = text.trim_end_matches('\0');
                    let (number, track_kind, frames, pregap, stored_pregap) = parse_track(text)?;
                    tracks.push(Track {
                        number,
                        kind: track_kind,
                        frames,
                        first_frame: next_frame,
                        pregap: if stored_pregap { pregap.min(frames) } else { 0 },
                    });
                    next_frame += frames.div_ceil(CD_TRACK_PADDING) * CD_TRACK_PADDING;
                }
                META_CD_OLD => {
                    return Err("this CHD uses the old binary CD track format; convert it with `chdman copy`".into())
                }
                META_GD_TRACK => return Err("this CHD is a Dreamcast GD-ROM, not a PS2 disc".into()),
                _ => {}
            }
        }
        let (layout, iso_bytes, iso_track) = if is_cd {
            if unit_bytes as usize != CD_FRAME_BYTES || hunk_bytes as usize % CD_FRAME_BYTES != 0 {
                return Err(damaged("a CD image without whole frames"));
            }
            let track = tracks
                .iter()
                .find(|track| track.data_offset().is_some())
                .ok_or("this CHD has no data track an ISO can be made of (audio only?)")?;
            let first = track.first_frame + track.pregap;
            let frames = track.frames - track.pregap;
            if (first + frames) * CD_FRAME_BYTES as u64 > logical_bytes {
                return Err(damaged("a track goes past the end of the data"));
            }
            let offset = track.data_offset().unwrap_or(0);
            (Layout::Cd { tracks: tracks.clone() }, frames * 2048, Some((first, frames, offset)))
        } else {
            // A `createdvd` image has a "DVD " entry and a `createhd` one a hard disk entry: both are raw sectors.
            (Layout::Raw, logical_bytes, None)
        };

        let info = Info {
            version,
            codecs: codecs.iter().filter(|&&codec| codec != 0).map(|&codec| codec_name(codec)).collect(),
            logical_bytes,
            hunk_bytes,
            unit_bytes,
            hunks,
            layout,
            iso_bytes,
            raw_sha1,
            sha1,
        };
        for &codec in &codecs {
            if codec != 0 && Codec::for_tag(codec).is_none() {
                return Err(format!("this CHD uses the codec \"{}\", which is not supported", codec_name(codec)));
            }
        }
        Ok(Self { path: path.to_path_buf(), info, codecs, map, metadata_hashes, iso_track })
    }

    /// The map of an uncompressed CHD: one 32-bit hunk number per hunk, 0 for a hunk of zeros.
    fn read_raw_map(file: &mut File, offset: u64, hunks: u32, hunk_bytes: u32) -> Result<Vec<MapEntry>, String> {
        let mut raw = vec![0u8; hunks as usize * 4];
        read_exact_at(file, offset, &mut raw)?;
        Ok(raw
            .chunks_exact(4)
            .map(|entry| {
                let block = u64::from(be32(entry));
                MapEntry {
                    kind: if block == 0 { MAP_ZERO } else { MAP_NONE },
                    length: hunk_bytes,
                    offset: block * u64::from(hunk_bytes),
                    crc: 0,
                }
            })
            .collect())
    }

    /// The compressed v5 map (MAME's `decompress_v5_map`), checked against its CRC-16.
    fn read_compressed_map(
        file: &mut File,
        offset: u64,
        hunks: u32,
        hunk_bytes: u32,
        unit_bytes: u32,
    ) -> Result<Vec<MapEntry>, String> {
        let mut head = [0u8; 16];
        read_exact_at(file, offset, &mut head)?;
        let map_bytes = be32(&head) as usize;
        let first_offset = be48(&head[4..]);
        let map_crc = be16(&head[10..]);
        let (length_bits, self_bits, parent_bits) = (i32::from(head[12]), i32::from(head[13]), i32::from(head[14]));
        if map_bytes > 64 << 20 || length_bits > 32 || self_bits > 32 || parent_bits > 32 {
            return Err(damaged("its map header is invalid"));
        }
        let mut compressed = vec![0u8; map_bytes];
        read_exact_at(file, offset + 16, &mut compressed)?;
        let mut bits = BitReader::new(&compressed);
        let mut tree = Huffman::new(16, 8);
        tree.import_rle(&mut bits)?;

        let mut kinds = vec![0u8; hunks as usize];
        let (mut last, mut repeat) = (0u8, 0u32);
        for kind in kinds.iter_mut() {
            if repeat > 0 {
                *kind = last;
                repeat -= 1;
                continue;
            }
            let value = tree.decode(&mut bits) as u8;
            if value == MAP_RLE_SMALL {
                *kind = last;
                repeat = 2 + tree.decode(&mut bits);
            } else if value == MAP_RLE_LARGE {
                *kind = last;
                repeat = 2 + 16 + (tree.decode(&mut bits) << 4);
                repeat += tree.decode(&mut bits);
            } else {
                *kind = value;
                last = value;
            }
        }

        let mut map = Vec::with_capacity(hunks as usize);
        let mut raw = Vec::with_capacity(hunks as usize * 12);
        let mut current = first_offset;
        let (mut last_self, mut last_parent) = (0u64, 0u64);
        let units_per_hunk = u64::from(hunk_bytes / unit_bytes);
        for (number, &kind) in kinds.iter().enumerate() {
            let (mut entry_kind, mut offset, mut length, mut crc) = (kind, current, 0u32, 0u16);
            match kind {
                MAP_CODEC_0..=MAP_CODEC_3 => {
                    length = bits.read(length_bits);
                    current += u64::from(length);
                    crc = bits.read(16) as u16;
                }
                MAP_NONE => {
                    length = hunk_bytes;
                    current += u64::from(length);
                    crc = bits.read(16) as u16;
                }
                MAP_SELF => {
                    offset = u64::from(bits.read(self_bits));
                    last_self = offset;
                }
                MAP_PARENT => {
                    offset = u64::from(bits.read(parent_bits));
                    last_parent = offset;
                }
                MAP_SELF_0 | MAP_SELF_1 => {
                    if kind == MAP_SELF_1 {
                        last_self += 1;
                    }
                    entry_kind = MAP_SELF;
                    offset = last_self;
                }
                MAP_PARENT_SELF => {
                    entry_kind = MAP_PARENT;
                    offset = number as u64 * u64::from(hunk_bytes) / u64::from(unit_bytes);
                    last_parent = offset;
                }
                MAP_PARENT_0 | MAP_PARENT_1 => {
                    if kind == MAP_PARENT_1 {
                        last_parent += units_per_hunk;
                    }
                    entry_kind = MAP_PARENT;
                    offset = last_parent;
                }
                _ => return Err(damaged(format!("hunk {number} has the unknown map type {kind}"))),
            }
            raw.push(entry_kind);
            raw.extend_from_slice(&length.to_be_bytes()[1..]);
            raw.extend_from_slice(&offset.to_be_bytes()[2..]);
            raw.extend_from_slice(&crc.to_be_bytes());
            map.push(MapEntry { kind: entry_kind, length, offset, crc });
        }
        if crc16(&raw) != map_crc {
            return Err(damaged("its map does not match its checksum"));
        }
        Ok(map)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The codecs.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    Zlib,
    Lzma,
    Huff,
    Flac,
    Zstd,
    CdZlib,
    CdLzma,
    CdFlac,
    CdZstd,
}

impl Codec {
    fn for_tag(codec: u32) -> Option<Self> {
        Some(match codec {
            CODEC_ZLIB => Self::Zlib,
            CODEC_LZMA => Self::Lzma,
            CODEC_HUFF => Self::Huff,
            CODEC_FLAC => Self::Flac,
            CODEC_ZSTD => Self::Zstd,
            CODEC_CD_ZLIB => Self::CdZlib,
            CODEC_CD_LZMA => Self::CdLzma,
            CODEC_CD_FLAC => Self::CdFlac,
            CODEC_CD_ZSTD => Self::CdZstd,
            _ => return None,
        })
    }
}

/// The dictionary size MAME's LZMA encoder picks for a hunk (`LzmaEncProps_Normalize` at level 9).
fn lzma_dictionary(bytes: u32) -> u32 {
    let mut dictionary = 1u32 << 26;
    if dictionary > bytes {
        for shift in 11..=30u32 {
            if bytes <= 2 << shift {
                dictionary = 2 << shift;
                break;
            }
            if bytes <= 3 << shift {
                dictionary = 3 << shift;
                break;
            }
        }
    }
    dictionary
}

/// The per-thread state of the decoders.
struct Decoders {
    zstd: ruzstd::decoding::FrameDecoder,
    scratch: Vec<u8>,
}

impl Decoders {
    fn new() -> Self {
        Self { zstd: ruzstd::decoding::FrameDecoder::new(), scratch: Vec::new() }
    }
}

fn inflate(source: &[u8], out: &mut [u8]) -> Result<(), String> {
    match miniz_oxide::inflate::decompress_slice_iter_to_slice(out, std::iter::once(source), false, true) {
        Ok(length) if length == out.len() => Ok(()),
        Ok(_) => Err(damaged("a deflate hunk is short")),
        Err(_) => Err(damaged("a deflate hunk does not decode")),
    }
}

fn unlzma(source: &[u8], out: &mut [u8]) -> Result<(), String> {
    use lzma_rs::decompress::raw::{LzmaDecoder, LzmaParams, LzmaProperties};
    let params = LzmaParams::new(
        LzmaProperties { lc: 3, lp: 0, pb: 2 },
        lzma_dictionary(out.len() as u32),
        Some(out.len() as u64),
    );
    let mut decoder = LzmaDecoder::new(params, None).map_err(|_| damaged("an LZMA hunk has bad parameters"))?;
    let mut input = source;
    let mut cursor = std::io::Cursor::new(&mut out[..]);
    decoder.decompress(&mut input, &mut cursor).map_err(|_| damaged("an LZMA hunk does not decode"))?;
    if cursor.position() as usize != cursor.get_ref().len() {
        return Err(damaged("an LZMA hunk is short"));
    }
    Ok(())
}

fn unzstd(decoders: &mut Decoders, source: &[u8], out: &mut [u8]) -> Result<(), String> {
    match decoders.zstd.decode_all(source, out) {
        Ok(length) if length == out.len() => Ok(()),
        Ok(_) => Err(damaged("a zstd hunk is short")),
        Err(_) => Err(damaged("a zstd hunk does not decode")),
    }
}

fn unhuff(source: &[u8], out: &mut [u8]) -> Result<(), String> {
    let mut bits = BitReader::new(source);
    let mut tree = Huffman::new(256, 16);
    tree.import_huffman(&mut bits)?;
    for byte in out.iter_mut() {
        *byte = tree.decode(&mut bits) as u8;
    }
    if bits.overflow() {
        return Err(damaged("a Huffman hunk is cut"));
    }
    Ok(())
}

/// FLAC block size of a hunk, as MAME's compressors choose it.
fn flac_block_size(bytes: usize) -> u32 {
    let mut block = (bytes / 4) as u32;
    while block > 2048 {
        block /= 2;
    }
    block
}

fn decode_plain(codec: Codec, decoders: &mut Decoders, source: &[u8], out: &mut [u8]) -> Result<(), String> {
    match codec {
        Codec::Zlib => inflate(source, out),
        Codec::Lzma => unlzma(source, out),
        Codec::Zstd => unzstd(decoders, source, out),
        Codec::Huff => unhuff(source, out),
        Codec::Flac => {
            let big_endian = match source.first() {
                Some(b'L') => false,
                Some(b'B') => true,
                _ => return Err(damaged("a FLAC hunk has no byte order")),
            };
            flac::decode_stereo16(&source[1..], flac_block_size(out.len()), out.len() / 4, big_endian, out)
                .map(|_| ())
                .map_err(damaged)
        }
        _ => Err(damaged("a CD codec in a plain hunk")),
    }
}

/// The CD codecs: the sectors with one codec, the subcode with deflate (zstd for `cdzs`), and the sync header and ECC
/// regenerated for the frames whose bit is set.
fn decode_cd(codec: Codec, decoders: &mut Decoders, source: &[u8], out: &mut [u8]) -> Result<(), String> {
    let frames = out.len() / CD_FRAME_BYTES;
    let sectors = frames * CD_SECTOR_BYTES;
    let mut buffer = std::mem::take(&mut decoders.scratch);
    buffer.resize(frames * CD_FRAME_BYTES, 0);
    let result = (|| {
        let (sector_data, subcode) = buffer.split_at_mut(sectors);
        let mut ecc_bits: &[u8] = &[];
        if codec == Codec::CdFlac {
            let used = flac::decode_stereo16(source, flac_block_size(sectors), sectors / 4, true, sector_data)
                .map_err(damaged)?;
            let rest = source.get(used..).ok_or_else(|| damaged("a CD FLAC hunk is cut"))?;
            inflate(rest, subcode)?;
        } else {
            let ecc_bytes = frames.div_ceil(8);
            let length_bytes = if out.len() < 65536 { 2 } else { 3 };
            let header = ecc_bytes + length_bytes;
            if source.len() < header {
                return Err(damaged("a CD hunk is cut"));
            }
            let mut base = (usize::from(source[ecc_bytes]) << 8) | usize::from(source[ecc_bytes + 1]);
            if length_bytes > 2 {
                base = (base << 8) | usize::from(source[ecc_bytes + 2]);
            }
            let base_data = source.get(header..header + base).ok_or_else(|| damaged("a CD hunk is cut"))?;
            let sub_data = &source[header + base..];
            ecc_bits = &source[..ecc_bytes];
            match codec {
                Codec::CdZlib => {
                    inflate(base_data, sector_data)?;
                    inflate(sub_data, subcode)?;
                }
                Codec::CdLzma => {
                    unlzma(base_data, sector_data)?;
                    inflate(sub_data, subcode)?;
                }
                _ => {
                    unzstd(decoders, base_data, sector_data)?;
                    unzstd(decoders, sub_data, subcode)?;
                }
            }
        }
        for frame in 0..frames {
            let target = &mut out[frame * CD_FRAME_BYTES..(frame + 1) * CD_FRAME_BYTES];
            target[..CD_SECTOR_BYTES]
                .copy_from_slice(&sector_data[frame * CD_SECTOR_BYTES..(frame + 1) * CD_SECTOR_BYTES]);
            target[CD_SECTOR_BYTES..]
                .copy_from_slice(&subcode[frame * CD_SUBCODE_BYTES..(frame + 1) * CD_SUBCODE_BYTES]);
            if ecc_bits.get(frame / 8).is_some_and(|bits| bits & (1 << (frame % 8)) != 0) {
                target[..12].copy_from_slice(&CD_SYNC_HEADER);
                ecc_generate(target);
            }
        }
        Ok(())
    })();
    decoders.scratch = buffer;
    result
}

/// Reads hunks of one CHD; each worker thread has its own.
struct HunkReader {
    chd: Arc<Chd>,
    file: File,
    decoders: Decoders,
    compressed: Vec<u8>,
}

impl HunkReader {
    fn new(chd: Arc<Chd>) -> Result<Self, String> {
        let file = File::open(&chd.path).map_err(|error| format!("{}: {error}", chd.path.display()))?;
        Ok(Self { chd, file, decoders: Decoders::new(), compressed: Vec::new() })
    }

    /// Decodes hunk `number` into `out` (one hunk long) and checks its CRC.
    fn read(&mut self, number: u32, out: &mut [u8]) -> Result<(), String> {
        self.read_at_depth(number, out, 0)
    }

    fn read_at_depth(&mut self, number: u32, out: &mut [u8], depth: u32) -> Result<(), String> {
        let entry = *self.chd.map.get(number as usize).ok_or_else(|| damaged("a hunk number is out of range"))?;
        match entry.kind {
            MAP_ZERO => {
                out.fill(0);
                return Ok(());
            }
            MAP_SELF => {
                if depth > 8 || entry.offset >= u64::from(self.chd.info.hunks) || entry.offset == u64::from(number) {
                    return Err(damaged(format!("hunk {number} copies an invalid hunk")));
                }
                return self.read_at_depth(entry.offset as u32, out, depth + 1);
            }
            MAP_PARENT => return Err("this CHD needs a parent CHD (a delta image); use a complete CHD".into()),
            _ => {}
        }
        self.compressed.resize(entry.length as usize, 0);
        read_exact_at(&mut self.file, entry.offset, &mut self.compressed)?;
        if entry.kind == MAP_NONE {
            if self.compressed.len() != out.len() {
                return Err(damaged(format!("hunk {number} has the wrong size")));
            }
            out.copy_from_slice(&self.compressed);
        } else {
            let tag = self.chd.codecs[usize::from(entry.kind)];
            let codec = Codec::for_tag(tag).ok_or_else(|| damaged(format!("hunk {number} uses a missing codec")))?;
            let compressed = std::mem::take(&mut self.compressed);
            let decoded = match codec {
                Codec::CdZlib | Codec::CdLzma | Codec::CdFlac | Codec::CdZstd => {
                    decode_cd(codec, &mut self.decoders, &compressed, out)
                }
                _ => decode_plain(codec, &mut self.decoders, &compressed, out),
            };
            self.compressed = compressed;
            decoded.map_err(|error| format!("{error} (hunk {number}, {})", codec_name(tag)))?;
        }
        // The CRC of an uncompressed CHD's hunk is not stored.
        if self.chd.codecs[0] != 0 && crc16(out) != entry.crc {
            return Err(damaged(format!("hunk {number} does not match its checksum")));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The public operations.

/// Reads the header, map and metadata of `path`, decoding no hunk: for a quick answer when a file is chosen.
pub fn info(path: &Path) -> Result<Info, String> {
    Chd::open(path).map(|chd| chd.info)
}

/// The ISO inside a CHD read in place, a few sectors at a time: to validate the disc (its `SYSTEM.CNF` and executable)
/// before converting gigabytes. Each hunk read is checked against its CRC.
pub struct Disc {
    reader: HunkReader,
    /// The hunk last decoded, kept because sector reads come in runs.
    cached: Option<(u32, Vec<u8>)>,
}

impl Disc {
    pub fn open(path: &Path) -> Result<Self, String> {
        let chd = Arc::new(Chd::open(path)?);
        Ok(Self { reader: HunkReader::new(chd)?, cached: None })
    }

    pub fn info(&self) -> &Info {
        &self.reader.chd.info
    }

    fn hunk(&mut self, number: u32) -> Result<&[u8], String> {
        if self.cached.as_ref().is_none_or(|(cached, _)| *cached != number) {
            let mut data = self.cached.take().map(|(_, data)| data).unwrap_or_default();
            data.resize(self.reader.chd.info.hunk_bytes as usize, 0);
            self.reader.read(number, &mut data)?;
            self.cached = Some((number, data));
        }
        Ok(self.cached.as_ref().map(|(_, data)| data.as_slice()).unwrap_or_default())
    }

    /// Reads `bytes` of the ISO starting at sector `lba` (2,048 bytes each); a read past the end stops there.
    pub fn read(&mut self, lba: u64, bytes: u64) -> Result<Vec<u8>, String> {
        let iso_bytes = self.info().iso_bytes;
        let start = lba.saturating_mul(2048).min(iso_bytes);
        let end = start.saturating_add(bytes).min(iso_bytes);
        let hunk_bytes = u64::from(self.info().hunk_bytes);
        let track = self.reader.chd.iso_track;
        let mut out = Vec::with_capacity((end - start) as usize);
        let mut position = start;
        while position < end {
            // Where this ISO byte lives in the raw data, and how many follow it in the same sector or hunk.
            let (raw, run) = match track {
                None => (position, hunk_bytes - position % hunk_bytes),
                Some((first, _, offset)) => {
                    let sector = position / 2048;
                    let inside = position % 2048;
                    ((first + sector) * CD_FRAME_BYTES as u64 + offset as u64 + inside, 2048 - inside)
                }
            };
            let take = run.min(end - position);
            let hunk = self.hunk((raw / hunk_bytes) as u32)?;
            let at = (raw % hunk_bytes) as usize;
            out.extend_from_slice(&hunk[at..at + take as usize]);
            position += take;
        }
        Ok(out)
    }
}

/// How many hunks each worker decodes at a time.
const HUNKS_PER_JOB: u32 = 32;

/// What a conversion produced.
#[derive(Debug, Clone)]
pub struct Summary {
    pub info: Info,
    pub iso_bytes: u64,
}

fn part_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(".part");
    output.with_file_name(name)
}

/// Writes the ISO inside `chd` to `output`, published only once every hunk and both SHA-1s check out. `progress` gets
/// the ISO bytes written so far and their total, and returns `false` to stop: a stopped or failed conversion leaves
/// nothing behind and fails ([`CANCELLED`] when stopped). The CHD is only read. `threads` 0 picks one per core but one.
pub fn extract_iso(
    chd: &Path,
    output: &Path,
    threads: usize,
    mut progress: impl FnMut(u64, u64) -> bool,
) -> Result<Summary, String> {
    if output == chd {
        return Err("the output must be a new file, not the CHD".into());
    }
    let image = Arc::new(Chd::open(chd)?);
    let part = part_path(output);
    let mut part_file = File::create(&part).map_err(|error| format!("{}: {error}", part.display()))?;
    let result = convert(&image, &mut part_file, threads, &mut progress)
        .and_then(|()| part_file.sync_all().map_err(|error| format!("writing the ISO: {error}")));
    drop(part_file);
    match result {
        Ok(()) => {
            if output.exists() {
                std::fs::remove_file(output).map_err(|error| format!("{}: {error}", output.display()))?;
            }
            std::fs::rename(&part, output).map_err(|error| format!("{}: {error}", output.display()))?;
            Ok(Summary { iso_bytes: image.info.iso_bytes, info: image.info.clone() })
        }
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            Err(error)
        }
    }
}

/// Turns the raw data of the CHD, in order, into the ISO's bytes.
struct IsoWriter<'a, W: Write> {
    out: std::io::BufWriter<&'a mut W>,
    /// For a CD: first frame, frames and data offset of the track.
    track: Option<(u64, u64, usize)>,
    /// Raw bytes seen so far.
    position: u64,
    written: u64,
}

impl<W: Write> IsoWriter<'_, W> {
    fn feed(&mut self, raw: &[u8]) -> Result<(), String> {
        let error = |error: std::io::Error| format!("writing the ISO: {error}");
        match self.track {
            None => self.out.write_all(raw).map_err(error)?,
            Some((first, frames, offset)) => {
                // Hunks always hold whole frames.
                let first_frame = self.position / CD_FRAME_BYTES as u64;
                for (index, frame) in raw.chunks_exact(CD_FRAME_BYTES).enumerate() {
                    let number = first_frame + index as u64;
                    if number >= first && number < first + frames {
                        self.out.write_all(&frame[offset..offset + 2048]).map_err(error)?;
                    }
                }
            }
        }
        let length = match self.track {
            None => raw.len() as u64,
            Some((first, frames, _)) => {
                let start = self.position / CD_FRAME_BYTES as u64;
                let end = start + (raw.len() / CD_FRAME_BYTES) as u64;
                end.min(first + frames).saturating_sub(start.max(first)) * 2048
            }
        };
        self.written += length;
        self.position += raw.len() as u64;
        Ok(())
    }
}

fn convert<W: Write>(
    chd: &Arc<Chd>,
    out: &mut W,
    threads: usize,
    progress: &mut impl FnMut(u64, u64) -> bool,
) -> Result<(), String> {
    let info = &chd.info;
    let threads = if threads == 0 {
        std::thread::available_parallelism().map_or(2, |count| count.get()).saturating_sub(1).clamp(1, 16)
    } else {
        threads.clamp(1, 64)
    };
    let jobs = info.hunks.div_ceil(HUNKS_PER_JOB) as usize;
    let window = threads * 4;
    let hunk = info.hunk_bytes as usize;
    let next_job = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    // Jobs written so far, so workers never run more than `window` jobs ahead of the writer.
    let written = (Mutex::new(0usize), Condvar::new());
    let mut writer = IsoWriter {
        out: std::io::BufWriter::with_capacity(4 << 20, out),
        track: chd.iso_track,
        position: 0,
        written: 0,
    };
    let mut sha1 = Sha1::new();
    let total = info.iso_bytes;
    if !progress(0, total) {
        return Err(CANCELLED.into());
    }

    let outcome = std::thread::scope(|scope| -> Result<(), String> {
        let (sender, receiver) = mpsc::channel::<(usize, Result<Vec<u8>, String>)>();
        for _ in 0..threads {
            let sender = sender.clone();
            let (next_job, stop, written) = (&next_job, &stop, &written);
            scope.spawn(move || {
                let mut reader = match HunkReader::new(chd.clone()) {
                    Ok(reader) => reader,
                    Err(error) => {
                        let _ = sender.send((usize::MAX, Err(error)));
                        return;
                    }
                };
                loop {
                    let job = next_job.fetch_add(1, Ordering::Relaxed);
                    if job >= jobs || stop.load(Ordering::Relaxed) {
                        return;
                    }
                    {
                        let (count, ready) = written;
                        let mut done = count.lock().unwrap_or_else(|poison| poison.into_inner());
                        while job >= *done + window && !stop.load(Ordering::Relaxed) {
                            done = ready.wait(done).unwrap_or_else(|poison| poison.into_inner());
                        }
                    }
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let first = job as u32 * HUNKS_PER_JOB;
                    let last = (first + HUNKS_PER_JOB).min(info.hunks);
                    let mut data = vec![0u8; (last - first) as usize * hunk];
                    let mut result = Ok(());
                    for (index, number) in (first..last).enumerate() {
                        result = reader.read(number, &mut data[index * hunk..(index + 1) * hunk]);
                        if result.is_err() {
                            break;
                        }
                    }
                    if sender.send((job, result.map(|()| data))).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);

        let halt = |message: String| {
            stop.store(true, Ordering::Relaxed);
            written.1.notify_all();
            Err(message)
        };
        let mut pending = BTreeMap::new();
        let mut next = 0usize;
        let mut remaining_raw = info.logical_bytes;
        while next < jobs {
            let Ok((job, result)) = receiver.recv() else {
                return halt(damaged("a worker stopped"));
            };
            match result {
                Ok(data) => {
                    pending.insert(job, data);
                }
                Err(error) => return halt(error),
            }
            while let Some(data) = pending.remove(&next) {
                // The last hunk may run past the logical size: only that much is hashed and converted.
                let used = (data.len() as u64).min(remaining_raw) as usize;
                remaining_raw -= used as u64;
                sha1.update(&data[..used]);
                if let Err(error) = writer.feed(&data[..used]) {
                    return halt(error);
                }
                next += 1;
                *written.0.lock().unwrap_or_else(|poison| poison.into_inner()) = next;
                written.1.notify_all();
                if !progress(writer.written.min(total), total) {
                    return halt(CANCELLED.into());
                }
            }
        }
        Ok(())
    });
    outcome?;
    writer.out.flush().map_err(|error| format!("writing the ISO: {error}"))?;
    drop(writer);

    let raw_sha1 = sha1.finish();
    if raw_sha1 != info.raw_sha1 {
        return Err(damaged("its data does not match its SHA-1"));
    }
    let mut hashes = chd.metadata_hashes.clone();
    hashes.sort();
    let mut combined = Sha1::new();
    combined.update(&raw_sha1);
    for entry in &hashes {
        combined.update(entry);
    }
    if combined.finish() != info.sha1 {
        return Err(damaged("its metadata does not match its SHA-1"));
    }
    progress(total, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flac::tests::{frame as flac_frame, BitWriter};

    /// What a test hunk is stored as.
    enum Stored {
        Codec(u8, Vec<u8>),
        None,
        SelfOf(u32),
    }

    /// Builds a v5 CHD: `hunks` are the raw hunks and how each is stored; the map is written with a flat Huffman tree.
    fn build_chd(
        codecs: [&[u8; 4]; 4],
        hunk_bytes: u32,
        unit_bytes: u32,
        hunks: &[(Vec<u8>, Stored)],
        metadata: &[(&[u8; 4], Vec<u8>)],
    ) -> Vec<u8> {
        let mut file = vec![0u8; HEADER_V5_BYTES];
        // Metadata first, right after the header.
        let meta_offset = if metadata.is_empty() { 0 } else { file.len() as u64 };
        for (index, (kind, data)) in metadata.iter().enumerate() {
            let next = if index + 1 == metadata.len() { 0 } else { (file.len() + 16 + data.len()) as u64 };
            file.extend_from_slice(*kind);
            file.push(METADATA_CHECKSUM_FLAG);
            file.extend_from_slice(&(data.len() as u32).to_be_bytes()[1..]);
            file.extend_from_slice(&next.to_be_bytes());
            file.extend_from_slice(data);
        }
        let first_offset = file.len() as u64;
        for (raw, stored) in hunks {
            match stored {
                Stored::Codec(_, bytes) => file.extend_from_slice(bytes),
                Stored::None => file.extend_from_slice(raw),
                Stored::SelfOf(_) => {}
            }
        }
        // The map: 16 codes of 4 bits each, so a type is written as its own 4-bit value.
        let mut bits = BitWriter::default();
        for _ in 0..16 {
            bits.put(4, 4);
        }
        for (_, stored) in hunks {
            let kind = match stored {
                Stored::Codec(index, _) => u64::from(*index),
                Stored::None => u64::from(MAP_NONE),
                Stored::SelfOf(_) => u64::from(MAP_SELF),
            };
            bits.put(kind, 4);
        }
        let (length_bits, self_bits) = (24u8, 16u8);
        let mut raw_map = Vec::new();
        let mut offset = first_offset;
        for (raw, stored) in hunks {
            match stored {
                Stored::Codec(index, bytes) => {
                    bits.put(bytes.len() as u64, u32::from(length_bits));
                    bits.put(u64::from(crc16(raw)), 16);
                    raw_map.push(*index);
                    raw_map.extend_from_slice(&(bytes.len() as u32).to_be_bytes()[1..]);
                    raw_map.extend_from_slice(&offset.to_be_bytes()[2..]);
                    raw_map.extend_from_slice(&crc16(raw).to_be_bytes());
                    offset += bytes.len() as u64;
                }
                Stored::None => {
                    bits.put(u64::from(crc16(raw)), 16);
                    raw_map.push(MAP_NONE);
                    raw_map.extend_from_slice(&hunk_bytes.to_be_bytes()[1..]);
                    raw_map.extend_from_slice(&offset.to_be_bytes()[2..]);
                    raw_map.extend_from_slice(&crc16(raw).to_be_bytes());
                    offset += u64::from(hunk_bytes);
                }
                Stored::SelfOf(target) => {
                    bits.put(u64::from(*target), u32::from(self_bits));
                    raw_map.push(MAP_SELF);
                    raw_map.extend_from_slice(&[0, 0, 0]);
                    raw_map.extend_from_slice(&u64::from(*target).to_be_bytes()[2..]);
                    raw_map.extend_from_slice(&[0, 0]);
                }
            }
        }
        let map_offset = file.len() as u64;
        file.extend_from_slice(&(bits.bytes.len() as u32).to_be_bytes());
        file.extend_from_slice(&first_offset.to_be_bytes()[2..]);
        file.extend_from_slice(&crc16(&raw_map).to_be_bytes());
        file.extend_from_slice(&[length_bits, self_bits, 0, 0]);
        file.extend_from_slice(&bits.bytes);

        let logical: Vec<u8> = hunks.iter().flat_map(|(raw, _)| raw.clone()).collect();
        let raw_sha1 = Sha1::digest(&logical);
        let mut entries: Vec<[u8; 24]> = metadata
            .iter()
            .map(|(kind, data)| {
                let mut entry = [0u8; 24];
                entry[..4].copy_from_slice(*kind);
                entry[4..].copy_from_slice(&Sha1::digest(data));
                entry
            })
            .collect();
        entries.sort();
        let mut combined = Sha1::new();
        combined.update(&raw_sha1);
        for entry in &entries {
            combined.update(entry);
        }

        file[..8].copy_from_slice(b"MComprHD");
        file[8..12].copy_from_slice(&(HEADER_V5_BYTES as u32).to_be_bytes());
        file[12..16].copy_from_slice(&5u32.to_be_bytes());
        for (index, codec) in codecs.iter().enumerate() {
            file[16 + index * 4..20 + index * 4].copy_from_slice(*codec);
        }
        file[32..40].copy_from_slice(&(logical.len() as u64).to_be_bytes());
        file[40..48].copy_from_slice(&map_offset.to_be_bytes());
        file[48..56].copy_from_slice(&meta_offset.to_be_bytes());
        file[56..60].copy_from_slice(&hunk_bytes.to_be_bytes());
        file[60..64].copy_from_slice(&unit_bytes.to_be_bytes());
        file[64..84].copy_from_slice(&raw_sha1);
        file[84..104].copy_from_slice(&combined.finish());
        file
    }

    fn lzma_raw(data: &[u8]) -> Vec<u8> {
        let mut packed = Vec::new();
        let options = lzma_rs::compress::Options {
            unpacked_size: lzma_rs::compress::UnpackedSize::WriteToHeader(Some(data.len() as u64)),
        };
        lzma_rs::lzma_compress_with_options(&mut &data[..], &mut packed, &options).unwrap();
        packed[13..].to_vec() // a CHD keeps the stream without the 13-byte .lzma header
    }

    fn deflate(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec(data, 6)
    }

    fn zstd(data: &[u8]) -> Vec<u8> {
        ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
    }

    fn pattern(length: usize, seed: u32) -> Vec<u8> {
        let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        (0..length)
            .map(|i| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                if i % 7 < 3 {
                    (state >> 16) as u8
                } else {
                    (i / 64) as u8
                }
            })
            .collect()
    }

    fn convert_file(bytes: &[u8]) -> (Result<Summary, String>, Option<Vec<u8>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (chd, iso) = (dir.path().join("game.chd"), dir.path().join("game.iso"));
        std::fs::write(&chd, bytes).unwrap();
        let result = extract_iso(&chd, &iso, 2, |_, _| true);
        assert!(!dir.path().join("game.iso.part").exists(), "the .part file never stays behind");
        assert_eq!(std::fs::read(&chd).unwrap(), bytes, "the CHD is only read");
        let data = std::fs::read(&iso).ok();
        (result, data, dir)
    }

    /// A small `huff` hunk: every byte value coded with 8 bits, so the data follows the tree as it is.
    fn huff(data: &[u8]) -> Vec<u8> {
        let mut bits = BitWriter::default();
        // Small tree: symbol 0 (a run) and symbol 9 (length 8) get one bit each.
        bits.put(1, 3); // length of symbol 0
        bits.put(7, 3); // the others start at 8
        bits.put(0, 3); // symbol 8: unused
        bits.put(1, 3); // symbol 9: one bit
        bits.put(7, 3); // the rest are unused
                        // Lengths of the 256 codes: 8 once, then a run of 255 more.
        bits.put(1, 1); // symbol 9 -> length 8 (code 1)
        bits.put(0, 1); // a run (code 0)
        bits.put(7, 3);
        bits.put(246, 8);
        for &byte in data {
            bits.put(u64::from(byte), 8);
        }
        bits.bytes
    }

    #[test]
    fn crc16_matches_ccitt_false() {
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }

    #[test]
    fn a_dvd_chd_with_every_plain_codec_converts_and_verifies() {
        let hunk = 4096usize;
        let raws: Vec<Vec<u8>> = (0..7).map(|i| pattern(hunk, i)).collect();
        let hunks = vec![
            (raws[0].clone(), Stored::Codec(0, lzma_raw(&raws[0]))),
            (raws[1].clone(), Stored::Codec(1, deflate(&raws[1]))),
            (raws[2].clone(), Stored::Codec(2, zstd(&raws[2]))),
            (raws[3].clone(), Stored::Codec(3, huff(&raws[3]))),
            (raws[4].clone(), Stored::None),
            (raws[1].clone(), Stored::SelfOf(1)),
            (raws[6].clone(), Stored::Codec(1, deflate(&raws[6]))),
        ];
        let bytes = build_chd([b"lzma", b"zlib", b"zstd", b"huff"], hunk as u32, 2048, &hunks, &[(b"DVD ", vec![0])]);
        let (result, iso, dir) = convert_file(&bytes);
        let summary = result.unwrap();
        assert_eq!(summary.info.kind(), "DVD");
        let expected: Vec<u8> = hunks.iter().flat_map(|(raw, _)| raw.clone()).collect();
        assert_eq!(iso.unwrap(), expected);
        let mut disc = Disc::open(&dir.path().join("game.chd")).unwrap();
        assert_eq!(disc.read(1, 4096).unwrap(), expected[2048..6144], "a read across two hunks");
    }

    #[test]
    fn a_flac_hunk_decodes() {
        let left: Vec<i16> = (0..256).map(|i| (i * 37 - 4000) as i16).collect();
        let right: Vec<i16> = (0..256).map(|i| (i * 5 + 3) as i16).collect();
        let mut raw = Vec::new();
        for i in 0..256 {
            raw.extend_from_slice(&left[i].to_le_bytes());
            raw.extend_from_slice(&right[i].to_le_bytes());
        }
        let mut stored = vec![b'L'];
        stored.extend(flac_frame(&left, &right));
        let hunks = vec![(raw.clone(), Stored::Codec(0, stored))];
        let bytes = build_chd([b"flac", b"\0\0\0\0", b"\0\0\0\0", b"\0\0\0\0"], 1024, 512, &hunks, &[]);
        let (result, iso, _dir) = convert_file(&bytes);
        result.unwrap();
        assert_eq!(iso.unwrap(), raw);
    }

    /// A CD image of one cooked MODE1 track (as `chdman createcd` makes from an ISO), four frames per hunk.
    fn cd_frames(sectors: &[Vec<u8>]) -> Vec<Vec<u8>> {
        sectors
            .chunks(4)
            .map(|chunk| {
                let mut raw = vec![0u8; 4 * CD_FRAME_BYTES];
                for (index, sector) in chunk.iter().enumerate() {
                    raw[index * CD_FRAME_BYTES..index * CD_FRAME_BYTES + sector.len()].copy_from_slice(sector);
                }
                raw
            })
            .collect()
    }

    fn cd_hunk(codec: &[u8; 4], raw: &[u8], ecc_bits: u8) -> Vec<u8> {
        let frames = raw.len() / CD_FRAME_BYTES;
        let mut sectors = Vec::new();
        let mut subcode = Vec::new();
        for (index, frame) in raw.chunks_exact(CD_FRAME_BYTES).enumerate() {
            let mut sector = frame[..CD_SECTOR_BYTES].to_vec();
            if ecc_bits & (1 << index) != 0 {
                // The compressor drops what the decoder can regenerate.
                sector[..12].fill(0);
                sector[0x81C..0x8C8 + 104].fill(0);
            }
            sectors.extend(sector);
            subcode.extend_from_slice(&frame[CD_SECTOR_BYTES..]);
        }
        if codec == b"cdfl" {
            let samples: Vec<i16> =
                sectors.chunks_exact(2).map(|pair| i16::from_be_bytes([pair[0], pair[1]])).collect();
            let (left, right): (Vec<i16>, Vec<i16>) = samples.chunks_exact(2).map(|pair| (pair[0], pair[1])).unzip();
            let block = flac_block_size(frames * CD_SECTOR_BYTES) as usize;
            let mut stored = Vec::new();
            for (l, r) in left.chunks(block).zip(right.chunks(block)) {
                stored.extend(flac_frame(l, r));
            }
            stored.extend(deflate(&subcode));
            return stored;
        }
        let base = if codec == b"cdlz" { lzma_raw(&sectors) } else { deflate(&sectors) };
        let mut stored = vec![ecc_bits];
        stored.extend_from_slice(&(base.len() as u16).to_be_bytes());
        stored.extend(base);
        stored.extend(deflate(&subcode));
        stored
    }

    #[test]
    fn a_cd_chd_gives_the_iso_of_its_data_track() {
        let sectors: Vec<Vec<u8>> = (0..10).map(|i| pattern(2048, 100 + i)).collect();
        let raws = cd_frames(&sectors);
        let hunks = vec![
            (raws[0].clone(), Stored::Codec(0, cd_hunk(b"cdlz", &raws[0], 0))),
            (raws[1].clone(), Stored::Codec(1, cd_hunk(b"cdzl", &raws[1], 0))),
            (raws[2].clone(), Stored::Codec(2, cd_hunk(b"cdfl", &raws[2], 0))),
        ];
        let track = b"TRACK:1 TYPE:MODE1 SUBTYPE:NONE FRAMES:10 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0\0".to_vec();
        let bytes = build_chd(
            [b"cdlz", b"cdzl", b"cdfl", b"\0\0\0\0"],
            (4 * CD_FRAME_BYTES) as u32,
            CD_FRAME_BYTES as u32,
            &hunks,
            &[(b"CHT2", track)],
        );
        let (result, iso, dir) = convert_file(&bytes);
        let summary = result.unwrap();
        assert_eq!(summary.info.kind(), "CD");
        assert_eq!(summary.iso_bytes, 10 * 2048);
        assert_eq!(iso.unwrap(), sectors.concat(), "the padding frames of the track are left out");

        // Sectors read in place, across hunks and codecs, and cut at the end of the track.
        let mut disc = Disc::open(&dir.path().join("game.chd")).unwrap();
        assert_eq!(disc.read(3, 3 * 2048).unwrap(), sectors[3..6].concat());
        assert_eq!(disc.read(9, 10 * 2048).unwrap(), sectors[9]);
        assert_eq!(disc.read(2, 100).unwrap(), sectors[2][..100]);
        assert!(disc.read(10, 2048).unwrap().is_empty());
    }

    #[test]
    fn a_raw_mode1_sector_gets_its_sync_and_ecc_back() {
        let mut sector = vec![0u8; CD_FRAME_BYTES];
        sector[..12].copy_from_slice(&CD_SYNC_HEADER);
        sector[12..16].copy_from_slice(&[0x00, 0x02, 0x00, 0x01]);
        sector[16..16 + 2048].copy_from_slice(&pattern(2048, 7));
        ecc_generate(&mut sector);
        assert!(sector[0x81C..0x930].iter().any(|&byte| byte != 0));
        let mut raws = [vec![0u8; 4 * CD_FRAME_BYTES]];
        raws[0][..CD_FRAME_BYTES].copy_from_slice(&sector);
        let hunks = vec![(raws[0].clone(), Stored::Codec(0, cd_hunk(b"cdzl", &raws[0], 1)))];
        let track = b"TRACK:1 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:1 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0\0".to_vec();
        let bytes = build_chd(
            [b"cdzl", b"\0\0\0\0", b"\0\0\0\0", b"\0\0\0\0"],
            (4 * CD_FRAME_BYTES) as u32,
            CD_FRAME_BYTES as u32,
            &hunks,
            &[(b"CHT2", track)],
        );
        let (result, iso, _dir) = convert_file(&bytes);
        result.unwrap();
        assert_eq!(iso.unwrap(), sector[16..16 + 2048], "a raw track gives the 2,048 bytes after the header");
    }

    #[test]
    fn damage_is_caught_and_nothing_is_left() {
        let raws: Vec<Vec<u8>> = (0..3).map(|i| pattern(2048, 50 + i)).collect();
        let hunks: Vec<(Vec<u8>, Stored)> =
            raws.iter().map(|raw| (raw.clone(), Stored::Codec(0, deflate(raw)))).collect();
        let good = build_chd([b"zlib", b"\0\0\0\0", b"\0\0\0\0", b"\0\0\0\0"], 2048, 2048, &hunks, &[]);
        assert!(convert_file(&good).0.is_ok());

        // A changed byte inside a hunk breaks its CRC (or its deflate stream).
        let mut bad_hunk = good.clone();
        bad_hunk[HEADER_V5_BYTES + 40] ^= 0x55;
        let (result, iso, _dir) = convert_file(&bad_hunk);
        assert!(result.unwrap_err().contains("damaged"));
        assert!(iso.is_none());

        // A wrong raw SHA-1 in the header.
        let mut bad_sha = good.clone();
        bad_sha[70] ^= 1;
        assert!(convert_file(&bad_sha).0.unwrap_err().contains("SHA-1"));

        // Not a CHD, an old version, a parent CHD.
        assert_eq!(convert_file(b"PK\x03\x04 not a chd").0.unwrap_err(), "not a CHD file (too short)");
        let mut old = good.clone();
        old[15] = 4;
        assert!(convert_file(&old).0.unwrap_err().contains("version 4"));
        let mut child = good.clone();
        child[110] = 1;
        assert!(convert_file(&child).0.unwrap_err().contains("parent"));
    }

    #[test]
    fn a_stopped_conversion_leaves_nothing() {
        let raws: Vec<Vec<u8>> = (0..80).map(|i| pattern(2048, i)).collect();
        let hunks: Vec<(Vec<u8>, Stored)> =
            raws.iter().map(|raw| (raw.clone(), Stored::Codec(0, deflate(raw)))).collect();
        let bytes = build_chd([b"zlib", b"\0\0\0\0", b"\0\0\0\0", b"\0\0\0\0"], 2048, 2048, &hunks, &[]);
        let dir = tempfile::tempdir().unwrap();
        let (chd, iso) = (dir.path().join("game.chd"), dir.path().join("game.iso"));
        std::fs::write(&chd, &bytes).unwrap();
        let mut calls = 0;
        let result = extract_iso(&chd, &iso, 3, |_, _| {
            calls += 1;
            calls < 2
        });
        assert_eq!(result.unwrap_err(), CANCELLED);
        assert!(!iso.exists() && !dir.path().join("game.iso.part").exists());
    }
}
