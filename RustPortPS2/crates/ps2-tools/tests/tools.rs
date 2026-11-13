//! Tests of the installer tools. No game files: every image is built in memory, and the generator goldens come from the
//! Python tools these replaced (run on the same synthetic inputs).

use std::fs;
use std::path::{Path, PathBuf};

use ps2_tools::elfcheck::{inspect_bytes, EXPECTED_CRC};
use ps2_tools::hash::{blake2b_64_hex, fnv};
use ps2_tools::{iop_gen, iop_manifest, iso, vu1, vu1n_gen, xdelta};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ps2-tools-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn golden(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(path)
}

fn normalized(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

// ---- hashes -----------------------------------------------------------------------------------------------------------

#[test]
fn fnv_matches_the_microcode_hash() {
    assert_eq!(fnv(b""), 0x1465_0fb0_739d_0383);
    assert_eq!(fnv(b"abc"), 0xe168_0151_0db8_9efd);
    let all: Vec<u8> = (0..=255).collect();
    assert_eq!(fnv(&all), 0x16d1_73bd_fcda_e583);
}

#[test]
fn blake2b_with_an_eight_byte_digest_matches_python() {
    let long: Vec<u8> = (0..=255u8).cycle().take(768).collect();
    assert_eq!(blake2b_64_hex(b""), "e4a6a0577479b2b4");
    assert_eq!(blake2b_64_hex(b"abc"), "d8bb14d833d59559");
    assert_eq!(blake2b_64_hex(&long), "6cdb98d18e1d1970");
    // The block boundary: a full block is the last one, one byte more adds a block.
    assert_eq!(blake2b_64_hex(&[b'x'; 128]), "901ce374e8e12402");
    assert_eq!(blake2b_64_hex(&[b'y'; 129]), "7c617a149f5301e6");
}

// ---- verify-elf -------------------------------------------------------------------------------------------------------

fn tiny_elf() -> Vec<u8> {
    let mut data = vec![0u8; 128];
    data[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    let put16 = |data: &mut Vec<u8>, at: usize, value: u16| data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    let put32 = |data: &mut Vec<u8>, at: usize, value: u32| data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put16(&mut data, 16, 2); // ET_EXEC
    put16(&mut data, 18, 8); // EM_MIPS
    put32(&mut data, 20, 1);
    put32(&mut data, 24, 0x0010_0008); // entry
    put32(&mut data, 28, 52); // phoff
    put16(&mut data, 42, 32); // phentsize
    put16(&mut data, 44, 1); // phnum
    for (index, value) in [1u32, 96, 0x0010_0000, 0, 32, 32, 5, 16].into_iter().enumerate() {
        put32(&mut data, 52 + index * 4, value); // PT_LOAD, offset, vaddr, paddr, filesz, memsz, flags, align
    }
    data
}

fn one_function(start: u32, end: u32) -> Vec<(String, u32, u32)> {
    vec![("f".to_owned(), start, end)]
}

#[test]
fn a_valid_layout_does_not_claim_compatibility() {
    let report = inspect_bytes(&tiny_elf(), &one_function(0x0010_0008, 0x0010_0010)).unwrap();
    assert!(!report.known_edition(), "a made-up image is not the retail edition (CRC {:08X})", EXPECTED_CRC);
    assert_eq!(report.entry, 0x0010_0008);
    assert_eq!(report.mapped_functions, 1);
    let json = report.to_json();
    assert!(json.contains("\"layout_valid\": true") && json.contains("\"compatibility_verified\": false"));
    assert!(json.contains("\"known_edition\": false"));
}

#[test]
fn a_truncated_segment_is_refused() {
    let mut data = tiny_elf();
    data.truncate(data.len() - 8);
    let error = inspect_bytes(&data, &one_function(0x0010_0008, 0x0010_0010)).unwrap_err();
    assert_eq!(error, "Truncated or inconsistent segment");
}

#[test]
fn a_function_outside_the_segments_is_refused() {
    let error = inspect_bytes(&tiny_elf(), &one_function(0x0020_0000, 0x0020_0004)).unwrap_err();
    assert!(error.contains("outside the ELF"), "{error}");
}

#[test]
fn another_architecture_is_refused() {
    let mut data = tiny_elf();
    data[18..20].copy_from_slice(&62u16.to_le_bytes());
    let error = inspect_bytes(&data, &one_function(0x0010_0008, 0x0010_0010)).unwrap_err();
    assert!(error.contains("architecture"), "{error}");
    assert!(inspect_bytes(b"not an elf", &[]).unwrap_err().contains("ELF32"));
}

// ---- iso-extract ------------------------------------------------------------------------------------------------------

/// A one-file ISO9660 image: the volume descriptor at sector 16, the root directory at sector 18 and `HELLO.TXT;1` at 19.
fn tiny_iso() -> Vec<u8> {
    tiny_iso_with(b"HELLO.TXT;1", 5, 18)
}

/// The same image with another file name, file size and sector of the root directory's own `.` record.
fn tiny_iso_with(name: &[u8], size: u32, self_lba: u32) -> Vec<u8> {
    const SECTOR: usize = 2048;
    let mut image = vec![0u8; 22 * SECTOR];
    let record = |lba: u32, length: u32, flags: u8, name: &[u8]| -> Vec<u8> {
        let mut r = vec![0u8; 33 + name.len() + (name.len() + 1) % 2];
        r[0] = r.len() as u8;
        r[2..6].copy_from_slice(&lba.to_le_bytes());
        r[10..14].copy_from_slice(&length.to_le_bytes());
        r[25] = flags;
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    };
    let pvd = &mut image[16 * SECTOR..17 * SECTOR];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    let root = record(18, SECTOR as u32, 2, &[0]);
    pvd[156..156 + root.len()].copy_from_slice(&root);
    pvd[80..84].copy_from_slice(&22u32.to_le_bytes()); // volume space size

    let dir = &mut image[18 * SECTOR..19 * SECTOR];
    let mut offset = 0;
    for entry in [record(self_lba, 2048, 2, &[0]), record(18, 2048, 2, &[1]), record(19, size, 0, name)] {
        dir[offset..offset + entry.len()].copy_from_slice(&entry);
        offset += entry.len();
    }
    image[19 * SECTOR..19 * SECTOR + 5].copy_from_slice(b"hello");
    image[20 * SECTOR + 3] = 9; // a non-zero sector that belongs to no file: it goes to .disc_meta
    image
}

// GOW-Port: a damaged image gives a clear error, never a file outside the output folder, a panic or an endless recursion.
#[test]
fn a_damaged_iso_is_refused_with_a_clear_message() {
    let directory = scratch("iso-damaged");
    let run = |image: Vec<u8>, tag: &str| {
        let iso_path = directory.join(format!("{tag}.iso"));
        fs::write(&iso_path, image).unwrap();
        let output = directory.join(format!("out-{tag}"));
        fs::create_dir_all(&output).unwrap();
        iso::run(&iso::Options { iso: iso_path, output: Some(output), ..Default::default() }).map_err(|e| e.to_string())
    };
    // A name that climbs out of the output folder.
    let error = run(tiny_iso_with(b"..;1", 5, 18), "dotdot").unwrap_err();
    assert!(error.contains("damaged") && error.contains("invalid file name"), "{error}");
    assert!(run(tiny_iso_with(b"A\\B.TXT;1", 5, 18), "backslash").unwrap_err().contains("invalid file name"));
    // A file that runs past the end of the image (an incomplete copy).
    let error = run(tiny_iso_with(b"HELLO.TXT;1", 3 * 2048 + 1, 18), "cut").unwrap_err();
    assert!(error.contains("HELLO.TXT is cut"), "{error}");
    // ... unless it is a padding file.
    assert!(run(tiny_iso_with(b"DUMMY.DAT;1", 3 * 2048 + 1, 18), "dummy").is_ok());
    // A sub-directory that is the root again: recursion stops.
    let mut looped = tiny_iso_with(b"SUB;1", 2048, 18);
    let entry = 18 * 2048 + 34 + 34; // third record of the root: make it a directory pointing at the root
    looped[entry + 2..entry + 6].copy_from_slice(&18u32.to_le_bytes());
    looped[entry + 25] = 2;
    assert!(run(looped, "loop").unwrap_err().contains("nested deeper"));
    // An image too short for the volume descriptor.
    let short = directory.join("short.iso");
    let mut image = tiny_iso();
    image.truncate(16 * 2048 + 100);
    fs::write(&short, image).unwrap();
    let error = iso::scan(&mut fs::File::open(&short).unwrap()).unwrap_err().to_string();
    assert!(error.contains("cut"), "{error}");
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_iso_lists_extracts_and_records_its_layout() {
    let directory = scratch("iso");
    let iso_path = directory.join("tiny.iso");
    fs::write(&iso_path, tiny_iso()).unwrap();

    let (files, layer1) = iso::scan(&mut fs::File::open(&iso_path).unwrap()).unwrap();
    assert_eq!(files, vec![iso::DiscFile { name: "HELLO.TXT".into(), lba: 19, size: 5 }]);
    assert_eq!(layer1, 0);

    let output = directory.join("out");
    fs::create_dir_all(&output).unwrap();
    iso::run(&iso::Options { iso: iso_path, output: Some(output.clone()), layout: true, ..Default::default() })
        .unwrap();
    assert_eq!(fs::read(output.join("HELLO.TXT")).unwrap(), b"hello");
    assert_eq!(fs::read_to_string(output.join(".disc_layout")).unwrap(), "sectors 22\nlayer1 0\nfile 19 5 HELLO.TXT\n");
    // Sectors 16 (descriptor), 18 (directory) and 20 (the stray one) are not part of a file.
    let meta = fs::read(output.join(".disc_meta")).unwrap();
    let sectors: Vec<u32> = meta.chunks_exact(4 + 2048).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    assert_eq!(sectors, vec![16, 18, 20]);
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_file_that_is_not_an_iso_is_refused() {
    let directory = scratch("noiso");
    let path = directory.join("x.iso");
    fs::write(&path, vec![0u8; 40 * 2048]).unwrap();
    assert!(iso::scan(&mut fs::File::open(&path).unwrap()).is_err());
    let _ = fs::remove_dir_all(&directory);
}

// ---- vu1-manifest -----------------------------------------------------------------------------------------------------

/// A pseudo-random "ELF" (the manifest tools only look at its bytes) and a manifest that rebuilds one image from it.
fn manifest_fixture() -> (Vec<u8>, String, Vec<u8>) {
    let mut state = 0x1234_5678u32;
    let elf: Vec<u8> = (0..40_000)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    // Two pieces: 64 bytes of the ELF at micro address 0, 32 bytes at 0x100.
    let pieces = [(0x400usize, 64usize, 0usize), (0x900, 32, 0x100)];
    let mut image = vec![0u8; vu1::SIZE];
    let mut text = format!("V 1\nE {} {}\n", elf.len(), blake2b_64_hex(&elf));
    for (offset, length, address) in pieces {
        image[address..address + length].copy_from_slice(&elf[offset..offset + length]);
        text.push_str(&format!("R {offset:x} {length} {}\n", blake2b_64_hex(&elf[offset..offset + length])));
    }
    text.push_str("I 0:0 100:1\n");
    (elf, text, image)
}

#[test]
fn the_manifest_rebuilds_the_image_under_its_game_name_and_verifies() {
    let (elf, manifest, image) = manifest_fixture();
    let directory = scratch("manifest");
    let (elf_path, manifest_path, output) =
        (directory.join("game.elf"), directory.join("manifest.txt"), directory.join("vu1"));
    fs::write(&elf_path, &elf).unwrap();
    fs::write(&manifest_path, &manifest).unwrap();

    assert_eq!(vu1::apply_manifest(&elf_path, &manifest_path, &output).unwrap(), 0);
    let produced = output.join(format!("vu1_{:016x}.bin", fnv(&image)));
    assert_eq!(fs::read(&produced).unwrap(), image);
    // Applying again changes nothing; verifying against the same folder is a 100 % match.
    assert_eq!(vu1::apply_manifest(&elf_path, &manifest_path, &output).unwrap(), 0);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    assert_eq!(vu1::verify_manifest(&elf_path, &manifest_path, &output).unwrap(), 0);
    // The manifest holds no code: none of the image's bytes are in it.
    assert!(!manifest.contains(&image.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_foreign_elf_writes_nothing() {
    let (mut elf, manifest, _) = manifest_fixture();
    for byte in &mut elf[0x400..0x440] {
        *byte ^= 0xFF;
    }
    let directory = scratch("foreign");
    let (elf_path, manifest_path, output) =
        (directory.join("game.elf"), directory.join("manifest.txt"), directory.join("vu1"));
    fs::write(&elf_path, &elf).unwrap();
    fs::write(&manifest_path, &manifest).unwrap();
    assert_eq!(vu1::apply_manifest(&elf_path, &manifest_path, &output).unwrap(), 1);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_unsupported_manifest_version_is_an_error() {
    let directory = scratch("version");
    let (elf_path, manifest_path) = (directory.join("game.elf"), directory.join("manifest.txt"));
    fs::write(&elf_path, [0u8; 64]).unwrap();
    fs::write(&manifest_path, "V 9\n").unwrap();
    assert!(vu1::apply_manifest(&elf_path, &manifest_path, &directory.join("out"))
        .unwrap_err()
        .contains("unsupported version"));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn extending_a_manifest_records_only_elf_locations() {
    let directory = scratch("manifest-extend");
    let (elf_path, manifest_path, captures, output) = (
        directory.join("game.elf"),
        directory.join("manifest.txt"),
        directory.join("captures"),
        directory.join("extended.txt"),
    );
    let mut elf = vec![0u8; 64];
    elf[0x10..0x20].copy_from_slice(b"abcdefghijklmnop");
    elf[0x20..].fill(0xff);
    let mut capture = vec![0u8; vu1::SIZE];
    capture[..16].copy_from_slice(&elf[0x10..0x20]);
    fs::create_dir_all(&captures).unwrap();
    fs::write(&elf_path, &elf).unwrap();
    fs::write(&manifest_path, format!("V 1\nE {} {}\n", elf.len(), blake2b_64_hex(&elf))).unwrap();
    fs::write(captures.join("capture.bin"), &capture).unwrap();

    assert_eq!(vu1::extend_manifest(&elf_path, &manifest_path, &captures, &output).unwrap(), 0);
    let text = fs::read_to_string(&output).unwrap();
    assert!(text.contains("R 10 16 "), "{text}");
    assert!(text.contains("I 0:0"), "{text}");
    assert_eq!(vu1::verify_manifest(&elf_path, &output, &captures).unwrap(), 0);
    // The output is a reconstruction recipe, not the captured byte sequence.
    assert!(!text.contains("6162636465666768696a6b6c6d6e6f70"));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn extending_a_manifest_compacts_duplicate_pieces() {
    let directory = scratch("manifest-compact");
    let (elf_path, manifest_path, captures, output) = (
        directory.join("game.elf"),
        directory.join("manifest.txt"),
        directory.join("captures"),
        directory.join("compact.txt"),
    );
    let mut elf = vec![0u8; 64];
    elf[0x10..0x20].copy_from_slice(b"abcdefghijklmnop");
    let mut capture = vec![0u8; vu1::SIZE];
    capture[..16].copy_from_slice(&elf[0x10..0x20]);
    let fingerprint = blake2b_64_hex(&elf[0x10..0x20]);
    fs::create_dir_all(&captures).unwrap();
    fs::write(&elf_path, &elf).unwrap();
    fs::write(
        &manifest_path,
        format!("V 1\nE {} {}\nR 10 16 {fingerprint}\nR 10 16 {fingerprint}\nI 0:1\n", elf.len(), blake2b_64_hex(&elf)),
    )
    .unwrap();
    fs::write(captures.join("capture.bin"), &capture).unwrap();

    assert_eq!(vu1::extend_manifest(&elf_path, &manifest_path, &captures, &output).unwrap(), 0);
    let text = fs::read_to_string(&output).unwrap();
    assert_eq!(text.lines().filter(|line| line.starts_with("R ")).count(), 1, "{text}");
    assert!(text.contains("I 0:0"), "{text}");
    assert_eq!(vu1::verify_manifest(&elf_path, &output, &captures).unwrap(), 0);
    let _ = fs::remove_dir_all(&directory);
}

// ---- vu1-programs -----------------------------------------------------------------------------------------------------

/// An ELF with `.DVP.ovlytab` declaring two programs, their `.DVP.overlay.*` sections and one PT_LOAD segment.
fn dvp_elf() -> (Vec<u8>, Vec<u8>) {
    let mut payload = vec![0u8; 0x400];
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte = (index % 251) as u8 + 1; // never all zero
    }
    let base_vaddr = 0x0010_0000u32;
    let table_vaddr = base_vaddr + 0x400;
    // ovlytab entries: {name, EE address, micro address}.
    let mut table = Vec::new();
    for (ee, micro) in [(base_vaddr, 0u32), (base_vaddr + 0x100, 0x200)] {
        for value in [0u32, ee, micro] {
            table.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut shstr = vec![0u8];
    let mut name = |text: &str| {
        let at = shstr.len() as u32;
        shstr.extend_from_slice(text.as_bytes());
        shstr.push(0);
        at
    };
    let (n_tab, n_ov0, n_ov1, n_str) =
        (name(".DVP.ovlytab"), name(".DVP.overlay.0"), name(".DVP.overlay.1"), name(".shstrtab"));

    let mut file = vec![0u8; 52 + 32];
    let data_off = file.len() as u32;
    file.extend_from_slice(&payload);
    let table_off = file.len() as u32;
    file.extend_from_slice(&table);
    let shstr_off = file.len() as u32;
    file.extend_from_slice(&shstr);
    while file.len() % 4 != 0 {
        file.push(0);
    }
    let shoff = file.len() as u32;
    let header = |name: u32, offset: u32, size: u32| -> Vec<u8> {
        let mut h = Vec::new();
        for value in [name, 1, 0, 0, offset, size, 0, 0, 1, 0] {
            h.extend_from_slice(&value.to_le_bytes());
        }
        h
    };
    for h in [
        vec![0u8; 40],
        header(n_tab, table_off, table.len() as u32),
        header(n_ov0, data_off, 0x100),
        header(n_ov1, data_off + 0x100, 0x80),
        header(n_str, shstr_off, shstr.len() as u32),
    ] {
        file.extend_from_slice(&h);
    }
    file[0..4].copy_from_slice(b"\x7fELF");
    file[28..32].copy_from_slice(&52u32.to_le_bytes()); // phoff
    file[32..36].copy_from_slice(&shoff.to_le_bytes());
    file[42..44].copy_from_slice(&32u16.to_le_bytes()); // phentsize
    file[44..46].copy_from_slice(&1u16.to_le_bytes()); // phnum
    file[46..48].copy_from_slice(&40u16.to_le_bytes()); // shentsize
    file[48..50].copy_from_slice(&5u16.to_le_bytes()); // shnum
    file[50..52].copy_from_slice(&4u16.to_le_bytes()); // shstrndx
    for (index, value) in
        [1u32, data_off, base_vaddr, base_vaddr, 0x400 + table.len() as u32, 0x400 + table.len() as u32, 5, 4]
            .into_iter()
            .enumerate()
    {
        file[52 + index * 4..56 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    let _ = table_vaddr;
    (file, payload)
}

#[test]
fn the_declared_programs_come_from_the_overlay_table() {
    let (elf, _) = dvp_elf();
    let programs = vu1::declared_programs(&elf);
    assert_eq!(programs.len(), 2);
    assert_eq!((programs[0].0, programs[0].1, programs[0].3), (0, 0, 0x100));
    assert_eq!((programs[1].0, programs[1].1, programs[1].3), (1, 0x200, 0x80));
    assert_eq!(programs[1].2 - programs[0].2, 0x100, "program 1 starts 0x100 bytes after program 0 in the file");
    assert!(vu1::declared_programs(b"not an elf").is_empty());
}

#[test]
fn what_the_manifest_covers_is_removed_and_the_rest_is_written_once() {
    let (elf, payload) = dvp_elf();
    let directory = scratch("programs");
    let (elf_path, manifest_path, output) =
        (directory.join("game.elf"), directory.join("manifest.txt"), directory.join("vu1"));
    fs::write(&elf_path, &elf).unwrap();
    let programs = vu1::declared_programs(&elf);

    // Without covered pieces both programs come out, each at its micro-memory address.
    fs::write(&manifest_path, "V 1\n").unwrap();
    let images = vu1::static_images(&elf, &manifest_path).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(&images[0].1[0..0x100], &payload[0..0x100]);
    assert_eq!(&images[1].1[0x200..0x280], &payload[0x100..0x180]);

    // Cover the first program entirely: only the second remains.
    fs::write(&manifest_path, format!("V 1\nR {:x} 256 0000000000000000\n", programs[0].2)).unwrap();
    let images = vu1::static_images(&elf, &manifest_path).unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].0, 1);

    assert_eq!(vu1::apply_programs(&elf_path, &manifest_path, &output).unwrap(), 0);
    let name = format!("vu1s_{:016x}.bin", fnv(&images[0].1));
    assert_eq!(fs::read(output.join(&name)).unwrap(), images[0].1);
    assert_eq!(vu1::apply_programs(&elf_path, &manifest_path, &output).unwrap(), 0);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1, "idempotent");
    let _ = fs::remove_dir_all(&directory);
}

// ---- iop-gen ----------------------------------------------------------------------------------------------------------

#[test]
fn iop_gen_matches_the_python_tool_on_a_small_module() {
    let output = scratch("iopgen");
    iop_gen::run(&golden("iop"), &output, "").unwrap();
    for name in ["iopn_gold_1000.cpp", "iopn_registry.cpp"] {
        assert_eq!(normalized(&output.join(name)), normalized(&golden("iop/expected").join(name)), "{name}");
    }
    let _ = fs::remove_dir_all(&output);
}

fn modified(path: &Path) -> std::time::SystemTime {
    fs::metadata(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).modified().unwrap()
}

/// A pause longer than the file system's timestamp granularity, so that a rewrite is visible in `modified`.
fn let_timestamps_move() {
    std::thread::sleep(std::time::Duration::from_millis(150));
}

#[test]
fn write_if_changed_writes_only_what_differs() {
    let dir = scratch("writeifchanged");
    let file = dir.join("out.cpp");
    assert!(ps2_tools::write_if_changed(&file, "a\nb\n").unwrap(), "a new file is written");
    let first = modified(&file);
    let_timestamps_move();
    assert!(!ps2_tools::write_if_changed(&file, "a\nb\n").unwrap(), "the same text is not written again");
    assert_eq!(modified(&file), first);
    fs::write(&file, "a\r\nb\r\n").unwrap();
    let crlf = modified(&file);
    let_timestamps_move();
    assert!(!ps2_tools::write_if_changed(&file, "a\nb\n").unwrap(), "Windows line endings count as the same text");
    assert_eq!(modified(&file), crlf);
    assert!(ps2_tools::write_if_changed(&file, "a\nc\n").unwrap(), "different text is written");
    assert_eq!(fs::read_to_string(&file).unwrap(), "a\nc\n");
    fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
    assert!(ps2_tools::write_if_changed(&file, "a\nc\n").unwrap(), "a file that is not UTF-8 is replaced");
    assert!(
        ps2_tools::write_if_changed(&dir.join("missing").join("out.cpp"), "x").is_err(),
        "an unwritable path is an error"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// GOW-Port: a module of more than PART_CHUNKS chunks is written as part files that the main file's table points to; a
/// module that becomes shorter loses the parts it no longer needs.
#[test]
fn iop_gen_splits_a_long_module_into_part_files() {
    let dump = scratch("iopgen-long");
    let output = scratch("iopgen-long-out");
    // NOPs (sll $zero, $zero, 0) and a final jr $ra: 8 chunks of 512 instructions plus one more chunk.
    let words = 512 * iop_gen::PART_CHUNKS + 2;
    let mut code = vec![0u8; words * 4];
    code[(words - 2) * 4..(words - 1) * 4].copy_from_slice(&0x03E0_0008u32.to_le_bytes());
    fs::write(dump.join("long.bin"), &code).unwrap();
    fs::write(dump.join("modules.txt"), format!("long 10000 {:x} 1\n", code.len())).unwrap();
    iop_gen::run(&dump, &output, "").unwrap();
    let main = fs::read_to_string(output.join("iopn_long_10000.cpp")).unwrap();
    let part0 = fs::read_to_string(output.join("iopn_long_10000_p0.cpp")).unwrap();
    let part1 = fs::read_to_string(output.join("iopn_long_10000_p1.cpp")).unwrap();
    assert!(!output.join("iopn_long_10000_p2.cpp").exists(), "9 chunks make two parts");
    assert!(main.contains("const Chunk kChunks[] = {iopn_long_10000_chunk0,"), "the table names the external chunks");
    assert!(main.contains("uint32_t iopn_long_10000_chunk8(IopCpuState"), "the main file declares every chunk");
    assert!(!main.contains("goto L"), "no chunk body stays in the main file");
    assert!(part0.contains("uint32_t iopn_long_10000_chunk0(") && part0.contains("uint32_t iopn_long_10000_chunk7("));
    assert!(part1.contains("uint32_t iopn_long_10000_chunk8(") && !part1.contains("chunk7("));

    // The module shrinks to one chunk: written as a single file again, and its old parts are removed.
    let short = &code[..512 * 4];
    fs::write(dump.join("long.bin"), short).unwrap();
    fs::write(dump.join("modules.txt"), format!("long 10000 {:x} 1\n", short.len())).unwrap();
    iop_gen::run(&dump, &output, "").unwrap();
    assert!(!output.join("iopn_long_10000_p0.cpp").exists() && !output.join("iopn_long_10000_p1.cpp").exists());
    assert!(fs::read_to_string(output.join("iopn_long_10000.cpp")).unwrap().contains("uint32_t chunk0("));
    let _ = fs::remove_dir_all(&dump);
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn iop_gen_leaves_the_files_alone_when_nothing_changed() {
    let output = scratch("iopgen-twice");
    iop_gen::run(&golden("iop"), &output, "").unwrap();
    let names = ["iopn_gold_1000.cpp", "iopn_registry.cpp"];
    let first: Vec<_> = names.iter().map(|n| modified(&output.join(n))).collect();
    let_timestamps_move();
    iop_gen::run(&golden("iop"), &output, "").unwrap();
    for (name, before) in names.iter().zip(first) {
        assert_eq!(modified(&output.join(name)), before, "{name} was rewritten");
        assert_eq!(normalized(&output.join(name)), normalized(&golden("iop/expected").join(name)), "{name}");
    }
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn iop_gen_rewrites_only_the_module_whose_code_changed() {
    let dump = scratch("iopgen-dump");
    let output = scratch("iopgen-changed");
    let code = fs::read(golden("iop/gold.bin")).unwrap();
    fs::write(dump.join("gold.bin"), &code).unwrap();
    fs::write(dump.join("other.bin"), &code).unwrap();
    // The listing carries the module's hash; it is left as it is, so the registry does not change either.
    fs::write(dump.join("modules.txt"), "gold 1000 58 1234\nother 2000 58 1234\n").unwrap();
    iop_gen::run(&dump, &output, "").unwrap();
    let gold = output.join("iopn_gold_1000.cpp");
    let other = output.join("iopn_other_2000.cpp");
    let registry = output.join("iopn_registry.cpp");
    let (gold_time, other_time, registry_time) = (modified(&gold), modified(&other), modified(&registry));
    let other_before = fs::read_to_string(&other).unwrap();

    // The first instruction becomes a different one (the lowest bit of its first byte flips).
    let mut changed = code.clone();
    changed[0] ^= 1;
    fs::write(dump.join("other.bin"), &changed).unwrap();
    let_timestamps_move();
    iop_gen::run(&dump, &output, "").unwrap();
    assert_ne!(fs::read_to_string(&other).unwrap(), other_before, "the changed module must generate other code");
    assert_ne!(modified(&other), other_time, "the changed module is rewritten");
    assert_eq!(modified(&gold), gold_time, "the unchanged module is not rewritten");
    assert_eq!(modified(&registry), registry_time, "the registry did not change");
    let _ = fs::remove_dir_all(&dump);
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn iop_gen_reads_the_overrides_that_ship_with_it() {
    let overrides = iop_gen::parse_overrides(iop_gen::DEFAULT_OVERRIDES);
    assert!(overrides.get("libsd_irx").is_some_and(|offsets| offsets.contains(&0x456c)));
    assert!(iop_gen::parse_overrides("# comment only\n\nbad\n").is_empty());
}

// ---- iop-manifest -----------------------------------------------------------------------------------------------------

/// A tiny IRX (ET_SCE_IOPRELEXEC) built by hand: one PT_LOAD of 0x20 bytes (+0x20 of bss), the PT_SCE_IOPMOD header with a
/// text size of 0x20 and a REL section with an R_MIPS_32, an R_MIPS_26 and a HI16/LO16 pair.
fn synthetic_irx() -> Vec<u8> {
    fn put32(buffer: &mut Vec<u8>, value: u32) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    fn put16(buffer: &mut Vec<u8>, value: u16) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    let mut elf = Vec::new();
    elf.extend_from_slice(b"\x7fELF\x01\x01\x01");
    elf.resize(16, 0);
    for value in [0xFF80u16, 8] {
        put16(&mut elf, value);
    }
    for value in [1u32, 0x10, 52, 0xC0, 0] {
        put32(&mut elf, value); // version, entry, phoff, shoff, flags
    }
    for value in [52u16, 32, 2, 40, 3, 0] {
        put16(&mut elf, value); // ehsize, phentsize, phnum, shentsize, shnum, shstrndx
    }
    // PT_LOAD (file 0x80, vaddr 0) and PT_SCE_IOPMOD (file 0x70).
    for value in [1u32, 0x80, 0, 0, 0x20, 0x40, 5, 4] {
        put32(&mut elf, value);
    }
    for value in [0x7000_0080u32, 0x70, 0, 0, 16, 16, 4, 4] {
        put32(&mut elf, value);
    }
    elf.resize(0x70, 0);
    for value in [0u32, 0x10, 0, 0x20] {
        put32(&mut elf, value); // module info pointer, entry, gp, text size
    }
    // Text: a word with an addend, a jal, a lui/addiu pair.
    for value in [0x100u32, 0x0C00_0010, 0x3C04_0000, 0x2484_0010, 0, 0, 0, 0] {
        put32(&mut elf, value);
    }
    // REL entries at 0xA0: (offset, info) with symbol 0.
    for (offset, kind) in [(0u32, 2u32), (4, 4), (8, 5), (12, 6)] {
        put32(&mut elf, offset);
        put32(&mut elf, kind);
    }
    elf.resize(0xC0, 0);
    // Section headers: null, .text (SHF_ALLOC), .rel.text (SHT_REL, info = 1).
    let section = |kind: u32, flags: u32, offset: u32, size: u32, info: u32, entsize: u32| {
        let mut raw = Vec::new();
        for value in [0u32, kind, flags, 0, offset, size, 0, info, 4, entsize] {
            put32(&mut raw, value);
        }
        raw
    };
    elf.extend(section(0, 0, 0, 0, 0, 0));
    elf.extend(section(1, 2, 0x80, 0x20, 0, 0));
    elf.extend(section(9, 0, 0xA0, 32, 1, 8));
    elf
}

fn synthetic_entry(base: u32) -> iop_manifest::Entry {
    // The relocated text, computed by hand: addend + base, jal target + base, hi/lo of base + 0x10 with the carry.
    let mut text = Vec::new();
    for word in [
        0x100 + base,
        0x0C00_0000 | ((0x40 + base) >> 2),
        0x3C04_0000 | ((base + 0x10 + 0x8000) >> 16),
        0x2484_0000 | ((base + 0x10) & 0xFFFF),
        0,
        0,
        0,
        0,
    ] {
        text.extend_from_slice(&word.to_le_bytes());
    }
    iop_manifest::Entry { file: "TEST.IRX".to_owned(), base, text_size: 0x20, hash: fnv(&text) }
}

#[test]
fn an_irx_is_relocated_like_the_iop_loader_does() {
    let entry = synthetic_entry(0x28000);
    let text = iop_manifest::module_text(&synthetic_irx(), &entry).unwrap();
    assert_eq!(text.len(), 0x20);
    assert_eq!(u32::from_le_bytes(text[0..4].try_into().unwrap()), 0x28100);
    assert_eq!(u32::from_le_bytes(text[4..8].try_into().unwrap()), 0x0C00_0000 | (0x28040 >> 2));
    // base + 0x10 = 0x28010: the low half is 0x8010 (negative as a signed LO16), so HI16 carries to 3.
    assert_eq!(u32::from_le_bytes(text[8..12].try_into().unwrap()) & 0xFFFF, 3);
    assert_eq!(u32::from_le_bytes(text[12..16].try_into().unwrap()) & 0xFFFF, 0x8010);
}

#[test]
fn a_module_that_does_not_match_the_manifest_is_refused() {
    let irx = synthetic_irx();
    let mut wrong_hash = synthetic_entry(0x28000);
    wrong_hash.hash ^= 1;
    assert!(iop_manifest::module_text(&irx, &wrong_hash).unwrap_err().contains("hash"));
    // Another base gives other bytes, so the hash of the first base does not fit.
    let mut other_base = synthetic_entry(0x28000);
    other_base.base = 0x30000;
    assert!(iop_manifest::module_text(&irx, &other_base).is_err());
    let mut wrong_size = synthetic_entry(0x28000);
    wrong_size.text_size = 0x40;
    assert!(iop_manifest::module_text(&irx, &wrong_size).unwrap_err().contains("size"));
    assert!(iop_manifest::module_text(
        b"not an elf at all, just text that is long enough to read a header from....",
        &wrong_hash
    )
    .is_err());
}

#[test]
fn iop_manifest_apply_writes_what_the_runtime_dump_writes() {
    let directory = scratch("iopman");
    let (game, output) = (directory.join("game"), directory.join("dump"));
    fs::create_dir_all(game.join("IOP_MOD")).unwrap();
    fs::write(game.join("IOP_MOD").join("Test.Irx"), synthetic_irx()).unwrap();
    let entry = synthetic_entry(0x28000);
    let manifest = directory.join("iop.manifest");
    fs::write(
        &manifest,
        format!(
            "# comment\nV 1\nM TEST.IRX {:x} {:x} {:x}\nM MISSING.IRX 30000 20 1\n",
            entry.base, entry.text_size, entry.hash
        ),
    )
    .unwrap();

    assert_eq!(iop_manifest::apply_manifest(&game, &manifest, &output).unwrap(), 0);
    // The file name is the runtime's identifier (lowercase, non-alphanumerics as `_`), and the module list its format.
    let code = fs::read(output.join("test_irx.bin")).unwrap();
    assert_eq!(fnv(&code), entry.hash);
    assert_eq!(
        fs::read_to_string(output.join("modules.txt")).unwrap(),
        format!("test_irx 28000 20 {:x}\n", entry.hash)
    );
    assert!(!output.join("missing_irx.bin").exists());
    // iop-gen takes the result as its dump.
    let generated = directory.join("native");
    iop_gen::run(&output, &generated, "").unwrap();
    assert!(generated.join("iopn_registry.cpp").is_file());
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn the_committed_iop_manifest_is_well_formed() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/scus-97399/iop.manifest");
    let entries = iop_manifest::parse_manifest(&fs::read_to_string(path).unwrap()).unwrap();
    assert!(!entries.is_empty());
    let mut spans: Vec<(u32, u32)> = entries.iter().map(|e| (e.base, e.base + e.text_size as u32)).collect();
    spans.sort_unstable();
    for pair in spans.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "modules overlap: {pair:x?}");
    }
    assert!(spans.iter().all(|&(base, end)| base >= 0x10000 && end < 0x70000), "outside the module arena");
    assert!(iop_manifest::parse_manifest("M ONLY 1 2\n").is_err());
    assert!(iop_manifest::parse_manifest("V 2\n").is_err());
    assert!(iop_manifest::parse_manifest("M A.IRX 1 2 3\nM a.irx 4 5 6\n").is_err());
}

// ---- vu1n-gen ---------------------------------------------------------------------------------------------------------

fn run_vu1n(output: &Path, inputs: Vec<PathBuf>, parts: usize) -> String {
    vu1n_gen::run(&vu1n_gen::Options { output: output.to_path_buf(), inputs, heads: None, parts, cache: None }).unwrap()
}

fn run_vu1n_cached(output: &Path, inputs: Vec<PathBuf>, cache: &Path) -> String {
    vu1n_gen::run(&vu1n_gen::Options {
        output: output.to_path_buf(),
        inputs,
        heads: None,
        parts: 2,
        cache: Some(cache.to_path_buf()),
    })
    .unwrap()
}

/// A micro-memory image with the given (lower, upper) block written at each position.
fn image_with(block: &[(u32, u32)], positions: &[usize]) -> Vec<u8> {
    let mut image = vec![0u8; vu1n_gen_size()];
    for &position in positions {
        for (index, (lower, upper)) in block.iter().enumerate() {
            let at = position + index * 8;
            image[at..at + 4].copy_from_slice(&lower.to_le_bytes());
            image[at + 4..at + 8].copy_from_slice(&upper.to_le_bytes());
        }
    }
    image
}

const ADD_THEN_END: [(u32, u32); 2] =
    [(0x8000_033C, (0xF << 21) | (3 << 16) | (2 << 11) | (1 << 6) | 0x28), (0x8000_033C, (1 << 30) | 0x2FF)];
const MUL_THEN_END: [(u32, u32); 2] =
    [(0x8000_033C, (0xF << 21) | (3 << 16) | (2 << 11) | (1 << 6) | 0x2A), (0x8000_033C, (1 << 30) | 0x2FF)];

fn parts_text(output: &Path) -> String {
    (0..2).map(|k| normalized(&output.join(format!("vu1n_part{k}.cpp")))).collect::<Vec<_>>().join("\n----\n")
}

#[test]
fn vu1n_gen_incremental_output_is_the_full_run_output() {
    let directory = scratch("vu1ncache");
    let captures = directory.join("captures");
    fs::create_dir_all(&captures).unwrap();
    let cache = directory.join("cache.txt");
    fs::write(captures.join("m.bin"), image_with(&ADD_THEN_END, &[0x100])).unwrap();
    fs::write(captures.join("n.bin"), image_with(&MUL_THEN_END, &[0x300])).unwrap();
    let first = run_vu1n_cached(&directory.join("out"), vec![captures.clone()], &cache);
    assert!(first.contains("2 images, 2 distinct blocks"), "{first}");
    assert!(!first.contains("from the cache"), "nothing cached yet: {first}");

    // A new image that sorts before the cached ones holds the same block at another pc: a full run names the block after
    // that pc, so the incremental run must too.
    fs::write(captures.join("a.bin"), image_with(&ADD_THEN_END, &[0x2000])).unwrap();
    let second = run_vu1n_cached(&directory.join("out"), vec![captures.clone()], &cache);
    assert!(second.contains("(1 read, the rest from the cache)"), "{second}");
    let full = run_vu1n(&directory.join("full"), vec![captures.clone()], 2);
    assert!(full.contains("3 images, 2 distinct blocks"), "{full}");
    assert_eq!(parts_text(&directory.join("out")), parts_text(&directory.join("full")));
    assert!(parts_text(&directory.join("out")).contains("b2000_"), "the earliest image's pc names the block");

    // Nothing new: everything comes from the cache, same output.
    let third = run_vu1n_cached(&directory.join("out"), vec![captures.clone()], &cache);
    assert!(third.contains("(0 read, the rest from the cache)"), "{third}");
    assert_eq!(parts_text(&directory.join("out")), parts_text(&directory.join("full")));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn vu1n_gen_drops_the_cache_when_a_read_image_changes() {
    let directory = scratch("vu1ncache-stale");
    let captures = directory.join("captures");
    fs::create_dir_all(&captures).unwrap();
    let cache = directory.join("cache.txt");
    fs::write(captures.join("m.bin"), image_with(&ADD_THEN_END, &[0x100])).unwrap();
    run_vu1n_cached(&directory.join("out"), vec![captures.clone()], &cache);
    // Same name, other bytes (and size kept): the block it gave is gone. A full read must replace the cached one.
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(captures.join("m.bin"), image_with(&MUL_THEN_END, &[0x100])).unwrap();
    let again = run_vu1n_cached(&directory.join("out"), vec![captures.clone()], &cache);
    assert!(!again.contains("from the cache"), "a changed image invalidates the cache: {again}");
    let full = run_vu1n(&directory.join("full"), vec![captures.clone()], 2);
    assert!(full.contains("1 images, 1 distinct blocks"), "{full}");
    assert_eq!(parts_text(&directory.join("out")), parts_text(&directory.join("full")));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn vu1n_gen_matches_the_python_tool_on_a_small_image() {
    let output = scratch("vu1ngen");
    let summary = run_vu1n(&output, vec![golden("vu1n/image.bin")], 1);
    assert!(summary.contains("3 generated"), "{summary}");
    assert_eq!(normalized(&output.join("vu1n_part0.cpp")), normalized(&golden("vu1n/expected/vu1n_part0.cpp")));
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn the_same_code_at_another_position_is_the_same_block() {
    // One simple block (an FMAC add pair, then an E-bit pair) placed at two micro-memory addresses.
    let block: [(u32, u32); 2] =
        [(0x8000_033C, (0xF << 21) | (3 << 16) | (2 << 11) | (1 << 6) | 0x28), (0x8000_033C, (1 << 30) | 0x2FF)];
    let place = |positions: &[usize]| -> Vec<u8> {
        let mut image = vec![0u8; vu1n_gen_size()];
        for &position in positions {
            for (index, (lower, upper)) in block.iter().enumerate() {
                let at = position + index * 8;
                image[at..at + 4].copy_from_slice(&lower.to_le_bytes());
                image[at + 4..at + 8].copy_from_slice(&upper.to_le_bytes());
            }
        }
        image
    };
    let directory = scratch("reloc");
    fs::write(directory.join("once.bin"), place(&[0x100])).unwrap();
    fs::write(directory.join("twice.bin"), place(&[0x100, 0x2000])).unwrap();
    let once = run_vu1n(&directory.join("a"), vec![directory.join("once.bin")], 1);
    let twice = run_vu1n(&directory.join("b"), vec![directory.join("twice.bin")], 1);
    assert!(once.contains("1 distinct blocks") && twice.contains("1 distinct blocks"), "{once} / {twice}");

    let text = normalized(&directory.join("b/vu1n_part0.cpp"));
    assert!(!text.contains("pc=0x"), "relocated text has no absolute pcs");
    assert!(text.contains("n.pc +"), "pcs are relative to the block's entry");
    let _ = fs::remove_dir_all(&directory);
}

fn vu1n_gen_size() -> usize {
    vu1::SIZE
}

// GOW-Port (decision D4): a translation keeps the USA `.text`; it is accepted as the same code, not as the original.
#[test]
fn a_translation_with_the_usa_code_is_the_same_code() {
    let mut report = inspect_bytes(&tiny_elf(), &one_function(0x0010_0008, 0x0010_0010)).unwrap();
    assert_eq!(report.text_sha1, None, "the tiny image has no section table, so no .text");
    assert!(!report.same_code());
    assert!(report.to_json().contains("\"text_sha1\": null") && report.to_json().contains("\"same_code\": false"));

    report.text_sha1 = Some(ps2_elf::code_identity::USA_TEXT_SHA1.to_owned());
    assert!(!report.known_edition(), "another CRC: not the original disc");
    assert!(report.same_code(), "the USA .text: the same generated code");
    assert!(report.to_json().contains("\"same_code\": true"));

    report.text_sha1 = Some("0".repeat(40));
    assert!(!report.same_code(), "other code is refused");
}

// ---- xdelta -----------------------------------------------------------------------------------------------------------

fn varint(value: u64) -> Vec<u8> {
    let mut digits = vec![(value & 0x7F) as u8];
    let mut rest = value >> 7;
    while rest != 0 {
        digits.push((rest & 0x7F) as u8 | 0x80);
        rest >>= 7;
    }
    digits.reverse();
    digits
}

/// One VCDIFF window with the xdelta3 checksum of `target`.
fn vcdiff_window(
    indicator: u8,
    segment: Option<(u64, u64)>,
    target: &[u8],
    data: &[u8],
    inst: &[u8],
    addr: &[u8],
) -> Vec<u8> {
    let mut body = varint(target.len() as u64);
    body.push(0);
    for section in [data, inst, addr] {
        body.extend(varint(section.len() as u64));
    }
    body.extend(xdelta::adler32(target).to_be_bytes());
    for section in [data, inst, addr] {
        body.extend_from_slice(section);
    }
    let mut window = vec![indicator | 0x04];
    if let Some((length, position)) = segment {
        window.extend(varint(length));
        window.extend(varint(position));
    }
    window.extend(varint(body.len() as u64));
    window.extend(body);
    window
}

const XDELTA_BASE: &[u8] = b"abcdefghijklmnop";

/// A patch that uses every instruction kind and address mode: source, near, here (an overlapping copy of its own output),
/// same, run, a two-instruction code, and a window that copies from the target already written.
fn xdelta_patch() -> (Vec<u8>, Vec<u8>) {
    let first = b"abcdwxyzefghefghefghefghzzzz";
    let second = b"mnopmnop";
    let third = b"abcd!";
    let mut patch = vec![0xD6, 0xC3, 0xC4, 0x00, 0x04];
    patch.extend(varint(9));
    patch.extend_from_slice(b"base//out");
    patch.extend(vcdiff_window(0x01, Some((16, 0)), first, b"wxyzz", &[20, 5, 52, 44, 0, 4], &[0, 4, 4]));
    patch.extend(vcdiff_window(0x01, Some((16, 0)), second, b"", &[20, 116], &[12, 12]));
    patch.extend(vcdiff_window(0x02, Some((4, 0)), third, b"!", &[247], &[0]));
    (patch, [&first[..], second, third].concat())
}

#[test]
fn an_xdelta_patch_is_applied_window_by_window() {
    let dir = scratch("xdelta-ok");
    let (patch, expected) = xdelta_patch();
    fs::write(dir.join("patch.xdelta"), &patch).unwrap();
    fs::write(dir.join("base.iso"), XDELTA_BASE).unwrap();
    fs::write(dir.join("out.iso"), b"an older output is replaced").unwrap();
    let mut reports = Vec::new();
    let summary = xdelta::apply(&dir.join("patch.xdelta"), &dir.join("base.iso"), &dir.join("out.iso"), |bytes| {
        reports.push(bytes)
    })
    .unwrap();
    assert_eq!(summary, xdelta::Summary { windows: 3, bytes: expected.len() as u64 });
    assert_eq!(fs::read(dir.join("out.iso")).unwrap(), expected);
    assert_eq!(reports, [28, 36, 41]);
    assert!(!dir.join("out.iso.part").exists());
    assert!(xdelta::check_header(&dir.join("patch.xdelta")).is_ok());
    assert!(xdelta::check_header(&dir.join("base.iso")).unwrap_err().contains("not an xdelta3"));
}

#[test]
fn an_xdelta_patch_can_be_cancelled_and_leaves_nothing() {
    let dir = scratch("xdelta-cancel");
    let (patch, _) = xdelta_patch();
    fs::write(dir.join("patch.xdelta"), &patch).unwrap();
    fs::write(dir.join("base.iso"), XDELTA_BASE).unwrap();
    let error =
        xdelta::apply_cancellable(&dir.join("patch.xdelta"), &dir.join("base.iso"), &dir.join("out.iso"), |bytes| {
            bytes < 30
        })
        .unwrap_err();
    assert_eq!(error, xdelta::CANCELLED);
    assert!(!dir.join("out.iso").exists() && !dir.join("out.iso.part").exists());
}

#[test]
fn an_xdelta_patch_for_another_disc_fails_on_the_first_window_and_writes_nothing() {
    let dir = scratch("xdelta-base");
    let (patch, _) = xdelta_patch();
    fs::write(dir.join("patch.xdelta"), &patch).unwrap();
    fs::write(dir.join("base.iso"), b"ABCDEFGHIJKLMNOP").unwrap();
    let error =
        xdelta::apply(&dir.join("patch.xdelta"), &dir.join("base.iso"), &dir.join("out.iso"), |_| {}).unwrap_err();
    assert!(error.contains("first window") && error.contains("different ISO"), "{error}");
    assert!(!dir.join("out.iso").exists() && !dir.join("out.iso.part").exists());

    fs::write(dir.join("short.iso"), b"abcd").unwrap();
    let error =
        xdelta::apply(&dir.join("patch.xdelta"), &dir.join("short.iso"), &dir.join("out.iso"), |_| {}).unwrap_err();
    assert!(error.contains("past the end of the base disc"), "{error}");
}

#[test]
fn unsupported_or_damaged_xdelta_patches_are_explained() {
    let dir = scratch("xdelta-bad");
    fs::write(dir.join("base.iso"), XDELTA_BASE).unwrap();
    let (patch, _) = xdelta_patch();
    let run = |bytes: &[u8]| {
        fs::write(dir.join("patch.xdelta"), bytes).unwrap();
        let result = xdelta::apply(&dir.join("patch.xdelta"), &dir.join("base.iso"), &dir.join("out.iso"), |_| {});
        assert!(!dir.join("out.iso").exists() && !dir.join("out.iso.part").exists());
        result.unwrap_err()
    };
    let mut compressed = patch.clone();
    compressed[4] |= 0x01;
    assert!(run(&compressed).contains("-S none"));
    assert!(run(b"PK\x03\x04 a zip file").contains("not an xdelta3"));
    assert!(run(&patch[..patch.len() - 3]).contains("damaged"));
    assert!(run(&patch[..15]).contains("no windows"), "a header alone has no windows");
    assert!(run(&patch[..12]).contains("damaged"), "cut inside the application header");
    assert!(xdelta::apply(&dir.join("base.iso"), &dir.join("base.iso"), &dir.join("base.iso"), |_| {}).is_err());
    assert_eq!(fs::read(dir.join("base.iso")).unwrap(), XDELTA_BASE, "the base disc is never touched");
}

// ---- memory card ------------------------------------------------------------------------------------------------------

/// Compares the formatter with a card the PS2 BIOS formatted (in PCSX2), when one is given: `GOW_TEST_PCSX2_CARD=<.ps2>`.
/// The card is only read; it is the user's, so it never becomes a fixture.
#[test]
fn the_formatted_card_matches_a_bios_card_when_one_is_given() {
    use ps2_tools::memcard::{self, CardTime, RAW_PAGE_SIZE};
    let Ok(path) = std::env::var("GOW_TEST_PCSX2_CARD") else {
        return;
    };
    let bios = fs::read(&path).unwrap();
    let ours = memcard::format(CardTime::now_japan());
    let report = memcard::check(&bios).unwrap();
    assert!(report.formatted && report.ecc_errors.is_empty(), "{report:?}");
    assert_eq!(ours[..RAW_PAGE_SIZE], bios[..RAW_PAGE_SIZE], "superblock and its ECC");
    // The last four spare bytes vary between cards (erased or zero); the data and the ECC must match.
    for page in [16, 17] {
        let range = page * RAW_PAGE_SIZE..page * RAW_PAGE_SIZE + RAW_PAGE_SIZE - 4;
        assert_eq!(ours[range.clone()], bios[range], "indirect FAT cluster, page {page}");
    }
    let fat_entry = |card: &[u8], index: usize| {
        let page = 18 + index / 128;
        let at = page * RAW_PAGE_SIZE + index % 128 * 4;
        u32::from_le_bytes(card[at..at + 4].try_into().unwrap())
    };
    for index in (8100..8192).chain([1]) {
        let bios_entry = fat_entry(&bios, index);
        if bios_entry == 0x7FFF_FFFF || index >= 8135 {
            assert_eq!(fat_entry(&ours, index), bios_entry, "FAT entry {index}");
        }
    }
    let root = 82 * RAW_PAGE_SIZE;
    for (offset, length) in [(0x00, 2), (0x10, 8), (0x20, 4), (0x40, 32)] {
        for entry in [0, RAW_PAGE_SIZE] {
            let range = root + entry + offset..root + entry + offset + length;
            assert_eq!(ours[range.clone()], bios[range], "root directory entry field {offset:#x}");
        }
    }
}
