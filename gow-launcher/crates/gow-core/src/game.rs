//! Checks on the game files the user supplied and on the runner the installer built.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

/// XOR of all the 32-bit words of the ELF (the CRC PCSX2 uses to identify games).
pub const CRC_GOD_OF_WAR: u32 = 0xD638_5328;

pub const EXECUTABLE_NAME: &str = if cfg!(windows) { "gow-2005.exe" } else { "gow-2005" };
/// What the runner was called before it became `gow-2005`: builds made by older versions still carry it.
pub const LEGACY_EXECUTABLE_NAME: &str = if cfg!(windows) { "ps2EntryRunner.exe" } else { "ps2EntryRunner" };

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameState {
    pub exists: bool,
    /// The official ELF, or a modified ELF that matches the fingerprint the installer stored next to the runner.
    pub valid: bool,
    pub modified: bool,
    pub crc: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataState {
    pub ok: bool,
    pub iso: String,
    pub extracted: bool,
    pub chosen_iso_missing: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableState {
    pub exists: bool,
    pub executable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedPaths {
    pub elf: String,
    pub iso: String,
    pub executable: String,
}

pub fn elf_crc(bytes: &[u8]) -> u32 {
    bytes.as_chunks::<4>().0.iter().fold(0u32, |crc, word| crc ^ u32::from_le_bytes(*word))
}

fn read_trimmed(path: &Path) -> String {
    fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string()).unwrap_or_default()
}

pub fn check_game(elf: &str, executable: &str) -> GameState {
    let mut state = GameState::default();
    if elf.is_empty() {
        return state;
    }
    let Ok(payload) = fs::read(elf) else {
        return state;
    };
    state.exists = true;
    let crc = elf_crc(&payload);
    state.crc = format!("{crc:08X}");
    if !payload.starts_with(b"\x7FELF") {
        return state;
    }
    if crc == CRC_GOD_OF_WAR {
        state.valid = true;
        return state;
    }
    // The installer writes this fingerprint next to the EXE only after building it. A different edition of the game
    // must be rebuilt; sharing the serial is not enough.
    if !executable.is_empty() {
        let directory = Path::new(executable).parent().unwrap_or_else(|| Path::new("."));
        let fingerprint = read_trimmed(&directory.join("game-elf.sha256"));
        let sha = hex(&Sha256::digest(&payload));
        if fingerprint.len() == 64 && fingerprint.eq_ignore_ascii_case(&sha) {
            state.valid = true;
            state.modified = true;
        }
    }
    state
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// First `*.iso` (case-insensitive, by name) of a directory.
pub fn first_iso(directory: &Path) -> Option<PathBuf> {
    let mut isos: Vec<PathBuf> = fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("iso")))
        .collect();
    isos.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));
    isos.into_iter().next()
}

pub fn check_data(elf: &str, iso: &str) -> DataState {
    let directory = Path::new(elf).parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let mut image = if !iso.is_empty() && Path::new(iso).is_file() { iso.to_string() } else { String::new() };
    if image.is_empty() && iso.is_empty() && !elf.is_empty() {
        // Same order as the runtime: "../God of War.iso" relative to the ELF, or the first .iso of its directory.
        let sibling = directory.join("..").join("God of War.iso");
        image = if sibling.is_file() {
            fs::canonicalize(&sibling).map(|p| strip_verbatim(&p)).unwrap_or(sibling).to_string_lossy().into_owned()
        } else {
            first_iso(&directory).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
        };
    }
    let extracted = !elf.is_empty() && directory.join("GODOFWAR.TOC").exists() && directory.join("PART1.PAK").exists();
    DataState {
        ok: !image.is_empty() || extracted,
        chosen_iso_missing: !iso.is_empty() && image.is_empty(),
        iso: image,
        extracted,
    }
}

/// `\\?\C:\x` -> `C:\x` (canonicalize adds the verbatim prefix on Windows).
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

pub fn check_executable(path: &str) -> ExecutableState {
    if path.is_empty() {
        return ExecutableState::default();
    }
    let Ok(metadata) = fs::metadata(path) else {
        return ExecutableState::default();
    };
    let exists = metadata.is_file();
    ExecutableState { exists, executable: exists && is_executable(path, &metadata) }
}

#[cfg(windows)]
fn is_executable(path: &str, _metadata: &fs::Metadata) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ["exe", "com", "bat", "cmd"].iter().any(|known| ext.eq_ignore_ascii_case(known)))
}

#[cfg(not(windows))]
fn is_executable(_path: &str, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

/// The memory card image the game saves to: `Mcd001.ps2` next to the ELF (slot 1 of the IOP, as the runtime resolves it
/// when `GOW_MC0` is not set). The `mc0` folder next to it only holds the few `mc0:` file operations outside the card.
pub fn memory_card_file(elf: &str) -> String {
    if elf.is_empty() {
        return String::new();
    }
    Path::new(elf).parent().unwrap_or_else(|| Path::new("")).join("Mcd001.ps2").to_string_lossy().into_owned()
}

/// Where the texture replacements of an installation live: `<installation>/textures`, next to the `game` folder that
/// holds the ELF (`<installation>/game/SCUS_973.99`). When the ELF is not in a folder called `game` the `textures` folder
/// is next to the ELF. None while no ELF is set.
pub fn textures_folder(elf: &str) -> Option<PathBuf> {
    let folder = Path::new(elf.trim()).parent().filter(|folder| !folder.as_os_str().is_empty())?;
    let is_game_folder = folder.file_name().is_some_and(|name| name.eq_ignore_ascii_case("game"));
    let installation = if is_game_folder { folder.parent().unwrap_or(folder) } else { folder };
    Some(installation.join("textures"))
}

/// The folder the runner indexes (`GOW_TEXTURE_REPLACE`): PCSX2's `replacements`, inside [`textures_folder`].
pub fn texture_replacements_folder(elf: &str) -> Option<PathBuf> {
    textures_folder(elf).map(|folder| folder.join("replacements"))
}

/// Looks for `relative` in `start` and up to five parents.
fn find_upward(start: &Path, relative: &str) -> Option<PathBuf> {
    let mut directory = Some(start);
    for _ in 0..6 {
        let current = directory?;
        let candidate = current.join(relative);
        if candidate.exists() {
            return Some(clean(&candidate));
        }
        directory = current.parent();
    }
    None
}

fn clean(path: &Path) -> PathBuf {
    let mut parts = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            other => parts.push(other.as_os_str()),
        }
    }
    parts
}

/// Paths found next to the launcher, in a repository checkout above it, or in the default installation folder.
pub fn detected_paths(start: &Path, default_destination: &Path) -> DetectedPaths {
    let mut elf = find_upward(start, "game/SCUS_973.99").or_else(|| find_upward(start, "SCUS_973.99"));
    let find_named = |name: &str| {
        let exe = |relative: &str| find_upward(start, &format!("{relative}{name}"));
        find_upward(start, name)
            .or_else(|| exe("RustPortPS2/native/build-dev/ps2xRuntime/"))
            .or_else(|| exe("RustPortPS2/native/build/ps2xRuntime/"))
            .or_else(|| exe("RustPortPS2/native/build/ps2xRuntime/Release/"))
            .or_else(|| exe("build/ps2xRuntime/"))
    };
    let mut executable = find_named(EXECUTABLE_NAME).or_else(|| find_named(LEGACY_EXECUTABLE_NAME));
    // A manual installation in the default folder is not below the launcher.
    if elf.is_none() {
        let candidate = default_destination.join("game/SCUS_973.99");
        if candidate.is_file() {
            elf = Some(clean(&candidate));
        }
    }
    if executable.is_none() {
        let build = default_destination.join("build/ps2xRuntime");
        if let Some(candidate) =
            [EXECUTABLE_NAME, LEGACY_EXECUTABLE_NAME].iter().map(|name| build.join(name)).find(|c| c.is_file())
        {
            executable = Some(clean(&candidate));
        }
    }
    let iso = elf
        .as_ref()
        .and_then(|elf| elf.parent().and_then(first_iso))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    DetectedPaths {
        elf: elf.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        iso,
        executable: executable.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf_with_crc(target: u32) -> Vec<u8> {
        // 0x7F 'E' 'L' 'F' is the first word; the second word is chosen so the XOR of all words equals `target`.
        let first = u32::from_le_bytes(*b"\x7FELF");
        let mut bytes = b"\x7FELF".to_vec();
        bytes.extend_from_slice(&(first ^ target).to_le_bytes());
        bytes
    }

    #[test]
    fn crc_is_the_xor_of_little_endian_words() {
        assert_eq!(elf_crc(&[1, 0, 0, 0, 3, 0, 0, 0]), 2);
        assert_eq!(elf_crc(&[1, 0, 0, 0, 9, 9]), 1); // a trailing partial word is ignored
        assert_eq!(elf_crc(&elf_with_crc(CRC_GOD_OF_WAR)), CRC_GOD_OF_WAR);
    }

    #[test]
    fn the_official_elf_is_valid() {
        let directory = tempfile::tempdir().unwrap();
        let elf = directory.path().join("SCUS_973.99");
        fs::write(&elf, elf_with_crc(CRC_GOD_OF_WAR)).unwrap();
        let state = check_game(elf.to_str().unwrap(), "");
        assert!(state.exists && state.valid && !state.modified);
        assert_eq!(state.crc, "D6385328");
    }

    #[test]
    fn a_modified_elf_needs_the_runners_fingerprint() {
        let directory = tempfile::tempdir().unwrap();
        let elf = directory.path().join("SCUS_973.99");
        let payload = elf_with_crc(0x1234_5678);
        fs::write(&elf, &payload).unwrap();
        let runner = directory.path().join(EXECUTABLE_NAME);
        fs::write(&runner, b"x").unwrap();
        assert!(!check_game(elf.to_str().unwrap(), runner.to_str().unwrap()).valid);
        fs::write(directory.path().join("game-elf.sha256"), hex(&Sha256::digest(&payload)).to_uppercase()).unwrap();
        let state = check_game(elf.to_str().unwrap(), runner.to_str().unwrap());
        assert!(state.valid && state.modified);
    }

    #[test]
    fn a_file_that_is_not_an_elf_is_never_valid() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("SCUS_973.99");
        fs::write(&file, [0u8; 8]).unwrap();
        let state = check_game(file.to_str().unwrap(), "");
        assert!(state.exists && !state.valid);
        assert!(!check_game("", "").exists);
        assert!(!check_game("/no/such/file", "").exists);
    }

    #[test]
    fn data_comes_from_the_chosen_iso_a_sibling_or_extracted_files() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        fs::create_dir(&game).unwrap();
        let elf = game.join("SCUS_973.99");
        fs::write(&elf, b"x").unwrap();
        let elf = elf.to_str().unwrap();
        assert!(!check_data(elf, "").ok);

        fs::write(game.join("b.ISO"), b"x").unwrap();
        fs::write(game.join("a.iso"), b"x").unwrap();
        let found = check_data(elf, "");
        assert!(found.ok && found.iso.ends_with("a.iso"));

        let missing = check_data(elf, "/no/such.iso");
        assert!(!missing.ok && missing.chosen_iso_missing);

        fs::write(game.join("GODOFWAR.TOC"), b"x").unwrap();
        fs::write(game.join("PART1.PAK"), b"x").unwrap();
        assert!(check_data(elf, "/no/such.iso").extracted);
    }

    #[test]
    fn executable_state() {
        let directory = tempfile::tempdir().unwrap();
        let runner = directory.path().join(EXECUTABLE_NAME);
        assert_eq!(check_executable(runner.to_str().unwrap()), ExecutableState::default());
        fs::write(&runner, b"x").unwrap();
        assert!(check_executable(runner.to_str().unwrap()).exists);
        assert!(!check_executable("").exists);
    }

    #[test]
    fn detection_walks_up_and_falls_back_to_the_default_destination() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        let start = repository.join("gow-launcher/target");
        fs::create_dir_all(&start).unwrap();
        fs::create_dir_all(repository.join("game")).unwrap();
        fs::write(repository.join("game/SCUS_973.99"), b"x").unwrap();
        fs::create_dir_all(repository.join("RustPortPS2/native/build/ps2xRuntime")).unwrap();
        fs::write(repository.join(format!("RustPortPS2/native/build/ps2xRuntime/{EXECUTABLE_NAME}")), b"x").unwrap();
        let found = detected_paths(&start, &root.path().join("nowhere"));
        assert!(found.elf.ends_with("SCUS_973.99") && found.executable.ends_with(EXECUTABLE_NAME));

        let destination = root.path().join("dest");
        fs::create_dir_all(destination.join("game")).unwrap();
        fs::create_dir_all(destination.join("build/ps2xRuntime")).unwrap();
        fs::write(destination.join("game/SCUS_973.99"), b"x").unwrap();
        fs::write(destination.join("build/ps2xRuntime").join(EXECUTABLE_NAME), b"x").unwrap();
        let lonely = tempfile::tempdir().unwrap();
        let found = detected_paths(lonely.path(), &destination);
        assert!(found.elf.contains("dest") && found.executable.contains("dest"));
    }

    #[test]
    fn detection_still_finds_a_runner_built_under_the_old_name() {
        let destination = tempfile::tempdir().unwrap();
        fs::create_dir_all(destination.path().join("game")).unwrap();
        fs::create_dir_all(destination.path().join("build/ps2xRuntime")).unwrap();
        fs::write(destination.path().join("game/SCUS_973.99"), b"x").unwrap();
        fs::write(destination.path().join("build/ps2xRuntime").join(LEGACY_EXECUTABLE_NAME), b"x").unwrap();
        let lonely = tempfile::tempdir().unwrap();
        let found = detected_paths(lonely.path(), destination.path());
        assert!(found.executable.ends_with(LEGACY_EXECUTABLE_NAME));

        fs::write(destination.path().join("build/ps2xRuntime").join(EXECUTABLE_NAME), b"x").unwrap();
        let found = detected_paths(lonely.path(), destination.path());
        assert!(found.executable.ends_with(EXECUTABLE_NAME), "the new name wins over a leftover old build");
    }

    #[test]
    fn memory_card_sits_next_to_the_elf() {
        assert!(memory_card_file("/a/b/SCUS_973.99").replace('\\', "/").ends_with("/a/b/Mcd001.ps2"));
        assert_eq!(memory_card_file(""), "");
    }
}
