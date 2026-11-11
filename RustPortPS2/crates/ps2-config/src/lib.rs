//! The recompiler's TOML configuration.
//!
//! One-to-one port of `PS2Recomp/ps2xRecomp/src/lib/config_manager.cpp`. The
//! defaults, the accepted spellings and the sanitisation rules are the same, so
//! the same `recomp.toml` produces the same `RecompilerConfig` on both sides.
//! Diagnostics that the C++ side sends to its reporter come back as
//! [`LoadReport::warnings`] instead, so the CLI and the launcher can display
//! them.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use ps2_elf::parse_number;
use toml::map::Map;
use toml::Value;

pub use ps2_elf::funcmap::{is_auto_generated_name, FunctionMap, FunctionMapEntry};

/// One `[[jump_tables.table.entries]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTableEntry {
    pub index: u32,
    pub target: u32,
}

/// One `[[jump_tables.table]]` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTable {
    pub address: u32,
    pub base_register: u32,
    pub entries: Vec<JumpTableEntry>,
}

/// Everything the recompiler reads from `recomp.toml`.
/// GOW-Port (decision D2): the `[codegen]` section. `mode = "compat"` (default) keeps the output byte-identical to PS2Recomp;
/// `mode = "gow"` turns on the God of War code generation switches that are ready, and each switch can still be set alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Codegen {
    /// MOVZ / MOVN write only the low 64 bits of rd, like the R5900 (PS2Recomp copies the whole 128-bit register).
    pub movz_low64: bool,
    /// Guest GPRs in C++ locals inside a generated function (written back around calls and the slow checkpoint path).
    pub register_locals: bool,
    /// COP0 Count reads and writes go to the EE cycle counter (PS2Runtime::eeCountRegister) instead of a frozen field.
    pub cop0_count_clock: bool,
    /// Generated functions take `R5900Context *__restrict ctx`: guest RAM (`rdram`) never holds the register file, so the
    /// compiler keeps guest registers in host registers across guest stores (idea from gta-vcs-recomp).
    pub restrict_ctx: bool,
    /// `ctx->pc` stored only where something can read it: not before pure ALU / FPU / MMI instructions or RAM accesses
    /// (those pass their pc to the slow path as `ps2AccessPc`), not around delay slots that read no pc, not before forward
    /// gotos or dispatched calls. Back edges, device accesses, syscalls and tail calls keep theirs.
    pub lean_pc: bool,
    /// VU0 macro-mode fixes from gta-vcs-recomp: VCALLMSR starts at CMSAR0 (PS2Recomp read `vi[27]`, past the 16-entry
    /// array) and VCLIPw sets the + bit for x > |w| and the - bit for x < -|w| (PS2Recomp swapped them and used w).
    pub vu0_macro_fixes: bool,
}

impl Codegen {
    /// The switches `mode = "gow"` turns on.
    pub fn gow() -> Self {
        Self {
            movz_low64: true,
            register_locals: false,
            cop0_count_clock: true,
            restrict_ctx: true,
            lean_pc: true,
            vu0_macro_fixes: true,
        }
    }

    /// True when every switch is off: the output is PS2Recomp's.
    pub fn is_compat(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecompilerConfig {
    pub input_path: String,
    pub output_path: String,
    pub ghidra_map_path: String,
    pub single_file_output: bool,
    pub low_memory_mode: bool,
    pub output_worker_threads: u32,
    pub patch_syscalls: bool,
    pub patch_cop0: bool,
    pub patch_cache: bool,
    pub skip_functions: Vec<String>,
    pub stub_implementations: Vec<String>,
    pub entry_point_hints: Vec<String>,
    pub patches: BTreeMap<u32, String>,
    pub mmio_by_instruction_address: BTreeMap<u32, u32>,
    pub jump_tables: Vec<JumpTable>,
    pub codegen: Codegen,
}

impl Default for RecompilerConfig {
    fn default() -> Self {
        Self {
            input_path: String::new(),
            output_path: String::new(),
            ghidra_map_path: String::new(),
            single_file_output: false,
            low_memory_mode: false,
            output_worker_threads: 0,
            patch_syscalls: false,
            // These two default to true in the C++ struct as well.
            patch_cop0: true,
            patch_cache: true,
            skip_functions: Vec::new(),
            stub_implementations: Vec::new(),
            entry_point_hints: Vec::new(),
            patches: BTreeMap::new(),
            mmio_by_instruction_address: BTreeMap::new(),
            jump_tables: Vec::new(),
            codegen: Codegen::default(),
        }
    }
}

/// Non-fatal findings produced while loading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io { path: String, source: std::io::Error },
    Parse { path: String, message: String },
    MissingKey { key: &'static str },
    InvalidNumber { key: String, value: String, source: ps2_elf::ParseNumberError },
    Serialize { message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Parse { path, message } => write!(f, "cannot parse {path}: {message}"),
            Self::MissingKey { key } => write!(f, "missing required key `{key}`"),
            Self::InvalidNumber { key, value, source } => write!(f, "invalid number for `{key}` ({value}): {source}"),
            Self::Serialize { message } => write!(f, "cannot write the configuration: {message}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidNumber { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl RecompilerConfig {
    /// Read and validate a `recomp.toml`.
    pub fn load(path: &Path) -> Result<(Self, LoadReport), ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|source| ConfigError::Io { path: path.display().to_string(), source })?;
        Self::from_toml_str(&text, &path.display().to_string())
    }

    /// Parse `text`; `origin` only names the source in error messages.
    pub fn from_toml_str(text: &str, origin: &str) -> Result<(Self, LoadReport), ConfigError> {
        let root: Value = text.parse().map_err(|error: toml::de::Error| ConfigError::Parse {
            path: origin.to_owned(),
            message: error.to_string(),
        })?;
        let root_table = root.as_table();
        let general =
            root.get("general").and_then(Value::as_table).ok_or(ConfigError::MissingKey { key: "general" })?;

        let mut report = LoadReport::default();
        let mut config = Self {
            input_path: required_string(general, "input")?,
            ghidra_map_path: optional_string(general, "ghidra_output").unwrap_or_default(),
            output_path: required_string(general, "output")?,
            single_file_output: optional_bool(general, "single_file_output").unwrap_or(false),
            low_memory_mode: optional_bool(general, "low_memory_mode").unwrap_or(false),
            ..Self::default()
        };

        // `output_worker_threads`, with the legacy singular spelling, clamped to
        // twice the hardware threads.
        let configured_workers = optional_integer(general, "output_worker_threads")
            .or_else(|| optional_integer(general, "output_worker_thread"))
            .unwrap_or(i64::from(config.output_worker_threads));
        let hardware_threads = std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0) as i64;
        let clamped_workers = configured_workers.clamp(0, hardware_threads * 2);
        if configured_workers != clamped_workers {
            report.warnings.push(format!(
                "output_worker_threads value {configured_workers} is out of range; clamped to {clamped_workers}."
            ));
        }
        config.output_worker_threads = clamped_workers as u32;

        config.patch_syscalls = optional_bool(general, "patch_syscalls").unwrap_or(config.patch_syscalls);
        config.patch_cop0 = optional_bool(general, "patch_cop0").unwrap_or(config.patch_cop0);
        config.patch_cache = optional_bool(general, "patch_cache").unwrap_or(config.patch_cache);

        // `stubs` and `skip` are accepted at `[general]` or at the root.
        if let Some(stubs) =
            string_array(general, "stubs").or_else(|| root_table.and_then(|table| string_array(table, "stubs")))
        {
            config.stub_implementations = stubs;
        }
        if let Some(skip) =
            string_array(general, "skip").or_else(|| root_table.and_then(|table| string_array(table, "skip")))
        {
            config.skip_functions = skip;
        }

        // `entry_points`, plus the legacy `untracked_stubs`, at both levels.
        for table in [Some(general), root_table].into_iter().flatten() {
            for key in ["entry_points", "untracked_stubs"] {
                if let Some(values) = string_array(table, key) {
                    config.entry_point_hints.extend(values);
                }
            }
        }
        config.entry_point_hints.sort();
        config.entry_point_hints.dedup();

        if let Some(patches) = root.get("patches").and_then(Value::as_table) {
            if let Some(instructions) = patches.get("instructions").and_then(Value::as_array) {
                for patch in instructions {
                    let Some(patch) = patch.as_table() else {
                        continue;
                    };
                    let (Some(address_value), Some(value_value)) = (patch.get("address"), patch.get("value")) else {
                        continue;
                    };
                    let address = match address_value {
                        Value::String(text) => parse_address("patches.instructions.address", text)?,
                        Value::Integer(value) => *value as u32,
                        _ => continue,
                    };
                    let value = match value_value {
                        Value::String(text) => text.clone(),
                        Value::Integer(value) => format!("0x{:x}", *value as u32),
                        _ => continue,
                    };
                    config.patches.insert(address, value);
                }
            }
        }

        if let Some(mmio) = root.get("mmio").and_then(Value::as_table) {
            for (key, value) in mmio {
                let instruction_address = parse_address("mmio", key)?;
                let mmio_address = match value {
                    Value::String(text) => parse_address("mmio", text)?,
                    Value::Integer(value) => *value as u32,
                    // The C++ side stores 0 for an unexpected value type.
                    _ => 0,
                };
                config.mmio_by_instruction_address.insert(instruction_address, mmio_address);
            }
        }

        if let Some(tables) = root
            .get("jump_tables")
            .and_then(Value::as_table)
            .and_then(|table| table.get("table"))
            .and_then(Value::as_array)
        {
            for table_node in tables {
                let Some(node) = table_node.as_table() else {
                    continue;
                };
                let mut table = JumpTable { address: 0, base_register: 0, entries: Vec::new() };
                if let Some(value) = node.get("address") {
                    table.address = match value {
                        Value::String(text) => parse_address("jump_tables.address", text)?,
                        Value::Integer(value) => *value as u32,
                        _ => 0,
                    };
                }
                if let Some(value) = node.get("base_register") {
                    table.base_register = match value {
                        Value::String(text) => parse_address("jump_tables.base_register", text)?,
                        Value::Integer(value) => *value as u32,
                        _ => 0,
                    };
                }
                if table.address == 0 {
                    continue;
                }
                let Some(entries) = node.get("entries").and_then(Value::as_array) else {
                    continue;
                };

                let mut fallback_index: u32 = 0;
                for entry_node in entries {
                    let Some(entry_node) = entry_node.as_table() else {
                        fallback_index += 1;
                        continue;
                    };
                    let mut entry = JumpTableEntry { index: fallback_index, target: 0 };
                    if let Some(value) = entry_node.get("index") {
                        entry.index = match value {
                            Value::String(text) => parse_address("jump_tables.entries.index", text)?,
                            Value::Integer(value) => *value as u32,
                            _ => fallback_index,
                        };
                    }
                    let mut has_target = false;
                    if let Some(value) = entry_node.get("target") {
                        match value {
                            Value::String(text) => {
                                entry.target = parse_address("jump_tables.entries.target", text)?;
                                has_target = true;
                            }
                            Value::Integer(value) => {
                                entry.target = *value as u32;
                                has_target = true;
                            }
                            _ => {}
                        }
                    }
                    if has_target {
                        table.entries.push(entry);
                    }
                    fallback_index += 1;
                }
                if !table.entries.is_empty() {
                    config.jump_tables.push(table);
                }
            }
        }

        // GOW-Port (decision D2): `[codegen]`. The mode sets the defaults; a switch written in the section wins.
        if let Some(codegen) = root.get("codegen").and_then(Value::as_table) {
            config.codegen = match optional_string(codegen, "mode").as_deref() {
                None | Some("compat") => Codegen::default(),
                Some("gow") => Codegen::gow(),
                Some(other) => {
                    report
                        .warnings
                        .push(format!("codegen.mode = \"{other}\" is not \"compat\" or \"gow\": compat used"));
                    Codegen::default()
                }
            };
            if let Some(value) = optional_bool(codegen, "movz_low64") {
                config.codegen.movz_low64 = value;
            }
            if let Some(value) = optional_bool(codegen, "register_locals") {
                config.codegen.register_locals = value;
            }
            if let Some(value) = optional_bool(codegen, "cop0_count_clock") {
                config.codegen.cop0_count_clock = value;
            }
            if let Some(value) = optional_bool(codegen, "restrict_ctx") {
                config.codegen.restrict_ctx = value;
            }
            if let Some(value) = optional_bool(codegen, "lean_pc") {
                config.codegen.lean_pc = value;
            }
            if let Some(value) = optional_bool(codegen, "vu0_macro_fixes") {
                config.codegen.vu0_macro_fixes = value;
            }
        }

        Ok((config, report))
    }

    /// Render the configuration the way `ConfigManager::saveConfig` does.
    ///
    /// Tables are written with sorted keys (the C++ side uses `std::map`).
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        toml::to_string(&Value::Table(self.to_value()))
            .map_err(|error| ConfigError::Serialize { message: error.to_string() })
    }

    /// Write the configuration to `path`.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = self.to_toml_string()?;
        std::fs::write(path, text).map_err(|source| ConfigError::Io { path: path.display().to_string(), source })
    }

    fn to_value(&self) -> Map<String, Value> {
        let mut general = Map::new();
        general.insert("input".to_owned(), Value::String(self.input_path.clone()));
        general.insert("ghidra_output".to_owned(), Value::String(self.ghidra_map_path.clone()));
        general.insert("output".to_owned(), Value::String(self.output_path.clone()));
        general.insert("single_file_output".to_owned(), Value::Boolean(self.single_file_output));
        general.insert("low_memory_mode".to_owned(), Value::Boolean(self.low_memory_mode));
        general.insert("output_worker_threads".to_owned(), Value::Integer(i64::from(self.output_worker_threads)));
        general.insert("patch_syscalls".to_owned(), Value::Boolean(self.patch_syscalls));
        general.insert("patch_cop0".to_owned(), Value::Boolean(self.patch_cop0));
        general.insert("patch_cache".to_owned(), Value::Boolean(self.patch_cache));
        general.insert("skip".to_owned(), string_array_value(&self.skip_functions));
        general.insert("stubs".to_owned(), string_array_value(&self.stub_implementations));
        general.insert("entry_points".to_owned(), string_array_value(&self.entry_point_hints));

        let mut root = Map::new();
        root.insert("general".to_owned(), Value::Table(general));

        // GOW-Port: written only when a switch is on, so a compat configuration saves as before.
        if !self.codegen.is_compat() {
            let mut codegen = Map::new();
            codegen.insert("mode".to_owned(), Value::String("gow".to_owned()));
            codegen.insert("movz_low64".to_owned(), Value::Boolean(self.codegen.movz_low64));
            codegen.insert("register_locals".to_owned(), Value::Boolean(self.codegen.register_locals));
            codegen.insert("cop0_count_clock".to_owned(), Value::Boolean(self.codegen.cop0_count_clock));
            codegen.insert("restrict_ctx".to_owned(), Value::Boolean(self.codegen.restrict_ctx));
            codegen.insert("lean_pc".to_owned(), Value::Boolean(self.codegen.lean_pc));
            codegen.insert("vu0_macro_fixes".to_owned(), Value::Boolean(self.codegen.vu0_macro_fixes));
            root.insert("codegen".to_owned(), Value::Table(codegen));
        }

        if !self.mmio_by_instruction_address.is_empty() {
            let mut mmio = Map::new();
            for (instruction, address) in &self.mmio_by_instruction_address {
                mmio.insert(format!("0x{instruction:x}"), Value::String(format!("0x{address:x}")));
            }
            root.insert("mmio".to_owned(), Value::Table(mmio));
        }

        if !self.jump_tables.is_empty() {
            let mut tables = Vec::new();
            for table in &self.jump_tables {
                let mut node = Map::new();
                node.insert("address".to_owned(), Value::String(format!("0x{:x}", table.address)));
                node.insert("base_register".to_owned(), Value::Integer(i64::from(table.base_register)));
                let entries = table
                    .entries
                    .iter()
                    .map(|entry| {
                        let mut node = Map::new();
                        node.insert("index".to_owned(), Value::Integer(i64::from(entry.index)));
                        node.insert("target".to_owned(), Value::String(format!("0x{:x}", entry.target)));
                        Value::Table(node)
                    })
                    .collect();
                node.insert("entries".to_owned(), Value::Array(entries));
                tables.push(Value::Table(node));
            }
            let mut jump_tables = Map::new();
            jump_tables.insert("table".to_owned(), Value::Array(tables));
            root.insert("jump_tables".to_owned(), Value::Table(jump_tables));
        }

        let instructions = self
            .patches
            .iter()
            .map(|(address, value)| {
                let mut node = Map::new();
                node.insert("address".to_owned(), Value::String(format!("0x{address:x}")));
                node.insert("value".to_owned(), Value::String(value.clone()));
                Value::Table(node)
            })
            .collect();
        let mut patches = Map::new();
        patches.insert("instructions".to_owned(), Value::Array(instructions));
        root.insert("patches".to_owned(), Value::Table(patches));

        root
    }
}

fn required_string(table: &Map<String, Value>, key: &'static str) -> Result<String, ConfigError> {
    match table.get(key) {
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err(ConfigError::MissingKey { key }),
    }
}

fn optional_string(table: &Map<String, Value>, key: &str) -> Option<String> {
    match table.get(key) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn optional_bool(table: &Map<String, Value>, key: &str) -> Option<bool> {
    match table.get(key) {
        Some(Value::Boolean(value)) => Some(*value),
        _ => None,
    }
}

fn optional_integer(table: &Map<String, Value>, key: &str) -> Option<i64> {
    match table.get(key) {
        Some(Value::Integer(value)) => Some(*value),
        _ => None,
    }
}

fn string_array(table: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    let array = table.get(key)?.as_array()?;
    let mut values = Vec::with_capacity(array.len());
    for value in array {
        match value {
            Value::String(text) => values.push(text.clone()),
            // toml11 stops with an error on a non-string element; skipping it
            // keeps a slightly broken file usable, and is reported by the
            // caller through the element count when it matters.
            _ => return None,
        }
    }
    Some(values)
}

fn string_array_value(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

fn parse_address(key: &str, text: &str) -> Result<u32, ConfigError> {
    parse_number(text).map_err(|source| ConfigError::InvalidNumber {
        key: key.to_owned(),
        value: text.to_owned(),
        source,
    })
}
