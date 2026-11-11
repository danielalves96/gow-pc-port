//! Configuration tests: the repository's real template plus the sanitisation
//! rules of `ConfigManager::loadConfig`.

use std::path::PathBuf;

use ps2_config::{ConfigError, RecompilerConfig};

fn template_path() -> PathBuf {
    // crates/ps2-config -> crates -> RustPortPS2
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join("games/scus-97399/gow-port.toml")
}

fn load_template() -> RecompilerConfig {
    let (config, report) = RecompilerConfig::load(&template_path()).expect("the template must parse");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    config
}

#[test]
fn reads_the_repository_template() {
    let config = load_template();

    assert_eq!(config.input_path, "@ELF@");
    assert_eq!(config.output_path, "@OUT@");
    assert_eq!(config.ghidra_map_path, "@MAP@");
    assert!(!config.single_file_output);
    assert!(!config.low_memory_mode);
    assert_eq!(config.output_worker_threads, 0);
    assert!(!config.patch_syscalls);
    assert!(config.patch_cop0);
    assert!(config.patch_cache);

    assert_eq!(config.stub_implementations.len(), 78);
    assert!(config.skip_functions.is_empty());
    assert_eq!(config.entry_point_hints.len(), 327);
    assert_eq!(config.patches.len(), 129);
    assert_eq!(config.mmio_by_instruction_address.len(), 227);
    assert!(config.jump_tables.is_empty());

    assert_eq!(config.patches.get(&0x0016_9250).map(String::as_str), Some("0x3c013f11"));

    for stub in &config.stub_implementations {
        assert!(!stub.is_empty());
        if let Some((name, address)) = stub.split_once('@') {
            assert!(!name.is_empty(), "{stub}");
            assert!(address.starts_with("0x"), "{stub}");
        }
    }

    let mut sorted = config.entry_point_hints.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, config.entry_point_hints, "entry points must be sorted and unique");
}

#[test]
fn the_template_round_trips_through_the_writer() {
    let config = load_template();
    let text = config.to_toml_string().expect("serialises");
    let (again, report) = RecompilerConfig::from_toml_str(&text, "<round trip>").expect("re-parses");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(again, config);
}

#[test]
fn clamps_output_worker_threads() {
    let hardware = std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0) as u32;
    let text = "[general]\ninput = \"a\"\noutput = \"b\"\noutput_worker_threads = 100000\n";
    let (config, report) = RecompilerConfig::from_toml_str(text, "clamp").unwrap();
    assert_eq!(config.output_worker_threads, hardware * 2);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);

    let text = "[general]\ninput = \"a\"\noutput = \"b\"\noutput_worker_threads = -5\n";
    let (config, report) = RecompilerConfig::from_toml_str(text, "clamp").unwrap();
    assert_eq!(config.output_worker_threads, 0);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn accepts_the_legacy_worker_key() {
    let text = "[general]\ninput = \"a\"\noutput = \"b\"\noutput_worker_thread = 1\n";
    let (config, _) = RecompilerConfig::from_toml_str(text, "legacy").unwrap();
    assert_eq!(config.output_worker_threads, 1);
}

#[test]
fn stubs_and_skip_fall_back_to_the_root_table() {
    let text = "\
stubs = [\"sceCdRead@0x0026F2F8\", \"ret0\"]
skip = [\"sub_00100008\"]
[general]
input = \"a\"
output = \"b\"
";
    let (config, _) = RecompilerConfig::from_toml_str(text, "root").unwrap();
    assert_eq!(config.stub_implementations, ["sceCdRead@0x0026F2F8", "ret0"]);
    assert_eq!(config.skip_functions, ["sub_00100008"]);
}

#[test]
fn patches_accept_strings_and_integers() {
    let text = "\
[general]
input = \"a\"
output = \"b\"
[[patches.instructions]]
address = \"0x16944c\"
value = \"0x27bd0090\"
[[patches.instructions]]
address = 0x21e4f0
value = 0x8c077f04
";
    let (config, _) = RecompilerConfig::from_toml_str(text, "patches").unwrap();
    assert_eq!(config.patches.len(), 2);
    assert_eq!(config.patches.get(&0x0016_944c).map(String::as_str), Some("0x27bd0090"));
    assert_eq!(config.patches.get(&0x0021_e4f0).map(String::as_str), Some("0x8c077f04"));
}

#[test]
fn parses_jump_tables_and_skips_entries_without_a_target() {
    let text = "\
[general]
input = \"a\"
output = \"b\"
[[jump_tables.table]]
address = \"0x1000\"
base_register = 5
[[jump_tables.table.entries]]
index = 0
target = \"0x2000\"
[[jump_tables.table.entries]]
target = \"0x3000\"
[[jump_tables.table.entries]]
index = 7
";
    let (config, _) = RecompilerConfig::from_toml_str(text, "jump").unwrap();
    assert_eq!(config.jump_tables.len(), 1);
    let table = &config.jump_tables[0];
    assert_eq!(table.address, 0x1000);
    assert_eq!(table.base_register, 5);
    assert_eq!(table.entries.len(), 2);
    assert_eq!((table.entries[0].index, table.entries[0].target), (0, 0x2000));
    // The index falls back to the position when omitted.
    assert_eq!((table.entries[1].index, table.entries[1].target), (1, 0x3000));
}

#[test]
fn rejects_broken_input() {
    assert!(matches!(
        RecompilerConfig::from_toml_str("input = \"a\"\n", "no general"),
        Err(ConfigError::MissingKey { key: "general" })
    ));
    assert!(matches!(
        RecompilerConfig::from_toml_str("[general]\noutput = \"b\"\n", "no input"),
        Err(ConfigError::MissingKey { key: "input" })
    ));
    assert!(matches!(
        RecompilerConfig::from_toml_str(
            "[general]\ninput = \"a\"\noutput = \"b\"\n[[patches.instructions]]\naddress = \"0xnope\"\nvalue = \"0x1\"\n",
            "bad address"
        ),
        Err(ConfigError::InvalidNumber { .. })
    ));
    assert!(matches!(RecompilerConfig::from_toml_str("[general\n", "bad toml"), Err(ConfigError::Parse { .. })));
}

// GOW-Port (decision D2): the `[codegen]` section.
#[test]
fn the_codegen_section_picks_the_mode_and_single_switches() {
    let base = "[general]
input = \"a.elf\"
output = \"out/\"
";
    let (compat, _) = RecompilerConfig::from_toml_str(base, "test").unwrap();
    assert!(compat.codegen.is_compat(), "no section: PS2Recomp's output");
    assert!(!compat.to_toml_string().unwrap().contains("codegen"), "a compat configuration saves as before");

    let (gow, report) = RecompilerConfig::from_toml_str(
        &format!(
            "{base}[codegen]
mode = \"gow\"
"
        ),
        "test",
    )
    .unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(gow.codegen, ps2_config::Codegen::gow());
    let saved = gow.to_toml_string().unwrap();
    let (again, _) = RecompilerConfig::from_toml_str(&saved, "test").unwrap();
    assert_eq!(again.codegen, gow.codegen, "the section survives a save: {saved}");

    let (off, _) = RecompilerConfig::from_toml_str(
        &format!(
            "{base}[codegen]
mode = \"gow\"
movz_low64 = false
"
        ),
        "test",
    )
    .unwrap();
    assert!(!off.codegen.movz_low64, "a switch written in the section wins over the mode");
    assert!(off.codegen.restrict_ctx && off.codegen.lean_pc, "the other switches keep the mode's value");

    let (plain, _) = RecompilerConfig::from_toml_str(
        &format!(
            "{base}[codegen]
mode = \"gow\"
restrict_ctx = false
lean_pc = false
"
        ),
        "test",
    )
    .unwrap();
    assert!(!plain.codegen.restrict_ctx && !plain.codegen.lean_pc);
    let (plain_again, _) = RecompilerConfig::from_toml_str(&plain.to_toml_string().unwrap(), "test").unwrap();
    assert_eq!(plain_again.codegen, plain.codegen, "both switches survive a save");

    let (bad, report) = RecompilerConfig::from_toml_str(
        &format!(
            "{base}[codegen]
mode = \"fast\"
"
        ),
        "test",
    )
    .unwrap();
    assert!(bad.codegen.is_compat());
    assert!(report.warnings.iter().any(|w| w.contains("codegen.mode")), "{:?}", report.warnings);
}
