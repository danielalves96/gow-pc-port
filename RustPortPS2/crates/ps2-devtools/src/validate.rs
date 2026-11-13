//! `validate-config` and `validate-tests`: the checks CI runs without the game.

use std::collections::BTreeMap;
use std::path::Path;

use crate::util::{csv_row, find_root, option, parse_hex, GAME_PROFILE};

const LIBM_DOUBLE: [&str; 13] =
    ["sin", "cos", "tan", "atan", "atan2", "sqrt", "pow", "exp", "log", "fabs", "floor", "ceil", "fmod"];

fn python_list(row: &[String]) -> String {
    let items: Vec<String> =
        row.iter().map(|item| format!("'{}'", item.replace('\\', "\\\\").replace('\'', "\\'"))).collect();
    format!("[{}]", items.join(", "))
}

/// `(name, start, end)` of every function of `funcmap.csv` of the game profile; problems go to `errors`.
fn read_funcmap(path: &Path, errors: &mut Vec<String>) -> Vec<(String, i64, i64)> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            errors.push(format!("funcmap.csv: {error}"));
            return Vec::new();
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut lines = text.lines();
    let header = lines.next().map(csv_row);
    if header.as_deref() != Some(&["Name", "Start", "End", "Size"].map(String::from)[..]) {
        errors.push(format!(
            "funcmap.csv: header {}, expected Name,Start,End,Size",
            header.map_or_else(|| "None".to_owned(), |h| python_list(&h))
        ));
        return Vec::new();
    }
    let mut functions = Vec::new();
    for (index, line) in lines.enumerate() {
        let number = index + 2;
        if line.is_empty() {
            continue;
        }
        let row = csv_row(line);
        let parsed = (row.len() == 4)
            .then(|| (parse_hex(&row[1]), parse_hex(&row[2]), row[3].trim().parse::<i64>().ok()))
            .and_then(|(start, end, size)| Some((start?, end?, size?)));
        let Some((start, end, size)) = parsed else {
            errors.push(format!("funcmap.csv:{number}: malformed row: {}", python_list(&row)));
            continue;
        };
        let name = row[0].clone();
        if end - start != size {
            errors.push(format!("funcmap.csv:{number}: {name}: Size {size} != End - Start ({})", end - start));
        }
        if end <= start {
            errors.push(format!("funcmap.csv:{number}: {name}: End <= Start"));
        }
        functions.push((name, start, end));
    }

    let mut seen = std::collections::HashSet::new();
    for (i, (name, start, _)) in functions.iter().enumerate() {
        if !seen.insert(name.clone()) {
            errors.push(format!("funcmap.csv: repeated name {name}"));
        }
        if i > 0 && *start < functions[i - 1].1 {
            errors.push(format!("funcmap.csv: {name} (0x{start:08X}) is not sorted by address"));
        } else if i > 0 && *start < functions[i - 1].2 {
            errors.push(format!("funcmap.csv: {name} (0x{start:08X}) overlaps {}", functions[i - 1].0));
        }
    }
    functions
}

/// `name@0xADDRESS` -> (name, address).
fn parse_selector(text: &str) -> Option<(String, i64)> {
    let (name, address) = text.split_once('@')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_alphabetic() || first == '_' || first == '$') {
        return None;
    }
    if !chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.') {
        return None;
    }
    let digits = address.strip_prefix("0x")?;
    if digits.is_empty() || digits.len() > 8 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((name.to_owned(), i64::from_str_radix(digits, 16).ok()?))
}

fn selectors(config: &toml::Value, key: &str, errors: &mut Vec<String>) -> Vec<(String, i64)> {
    let values = config.get("general").and_then(|general| general.get(key)).and_then(|v| v.as_array());
    let mut result = Vec::new();
    for value in values.map(|v| v.as_slice()).unwrap_or(&[]) {
        match value.as_str().and_then(parse_selector) {
            Some(selector) => result.push(selector),
            None => {
                let shown = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
                errors.push(format!("gow-port.toml: {key}: '{shown}' does not have the name@0xADDRESS format"));
            }
        }
    }
    result
}

/// Every `X(name)` of the runtime's call list.
fn call_list_handlers(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut names = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'X' && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start && bytes.get(end) == Some(&b')') {
                names.push(text[start..end].to_owned());
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    names
}

/// `iop.manifest`: version line, `M FILE BASE SIZE HASH` records, no repeated module, no overlap inside the module arena.
fn check_iop_manifest(path: &Path, errors: &mut Vec<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        errors.push("iop.manifest: missing or unreadable".to_owned());
        return;
    };
    let mut spans: Vec<(u64, u64, String)> = Vec::new();
    let mut versioned = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[0] {
            "V" if fields.get(1) == Some(&"1") => versioned = true,
            "M" if fields.len() == 5 => {
                let hex: Vec<Option<u64>> = fields[2..].iter().map(|f| u64::from_str_radix(f, 16).ok()).collect();
                if let [Some(base), Some(size), Some(_)] = hex[..] {
                    let name = fields[1].to_ascii_lowercase();
                    if spans.iter().any(|s| s.2 == name) {
                        errors.push(format!("iop.manifest:{}: {} is listed twice", index + 1, fields[1]));
                    }
                    spans.push((base, base + size, name));
                } else {
                    errors.push(format!("iop.manifest:{}: BASE, SIZE and HASH must be hexadecimal", index + 1));
                }
            }
            _ => errors.push(format!("iop.manifest:{}: unexpected line: {line}", index + 1)),
        }
    }
    if !versioned {
        errors.push("iop.manifest: missing the `V 1` line".to_owned());
    }
    spans.sort();
    for pair in spans.windows(2) {
        if pair[0].1 > pair[1].0 {
            errors.push(format!("iop.manifest: {} overlaps {}", pair[0].2, pair[1].2));
        }
    }
}

pub fn validate_config(args: &[String]) -> i32 {
    let root = find_root(option(args, "--root").as_deref());
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let functions = read_funcmap(&root.join(GAME_PROFILE).join("funcmap.csv"), &mut errors);
    let starts: Vec<i64> = functions.iter().map(|f| f.1).collect();

    let text = std::fs::read_to_string(root.join(GAME_PROFILE).join("gow-port.toml")).unwrap_or_default();
    for marker in ["@ELF@", "@MAP@", "@OUT@"] {
        if !text.contains(marker) {
            errors.push(format!("gow-port.toml: missing the {marker} marker that the build script fills in"));
        }
    }
    let config = match text.parse::<toml::Value>() {
        Ok(config) => config,
        Err(error) => {
            errors.push(format!("gow-port.toml: invalid TOML: {error}"));
            toml::Value::Table(Default::default())
        }
    };

    let stubs = selectors(&config, "stubs", &mut errors);
    let entries = selectors(&config, "entry_points", &mut errors);

    for (key, items) in [("stubs", &stubs), ("entry_points", &entries)] {
        for (name, address) in items {
            let index = starts.partition_point(|start| start <= address) as i64 - 1;
            let function = (index >= 0).then(|| &functions[index as usize]);
            match function {
                Some(function) if *address < function.2 => {
                    if key == "stubs" && *address != function.1 {
                        warnings.push(format!(
                            "stubs: {name}@0x{address:08X} does not start a function (it is inside {})",
                            function.0
                        ));
                    }
                }
                _ => errors
                    .push(format!("{key}: {name}@0x{address:08X} does not fall inside any function of funcmap.csv")),
            }
        }
    }

    // God of War's libm is a software double ($a0 -> $v0); the runtime's handlers are float ($f12 -> $f0).
    // Replacing it returns the argument intact (the sine table came out with sin(x) = x).
    for (name, address) in &stubs {
        if LIBM_DOUBLE.contains(&name.as_str()) {
            errors.push(format!(
                "stubs: {name}@0x{address:08X} is the software double libm; the runtime's float handler does not respect its ABI"
            ));
        }
    }

    let mut by_address: BTreeMap<i64, Vec<&str>> = BTreeMap::new();
    for (name, address) in stubs.iter().chain(entries.iter()) {
        by_address.entry(*address).or_default().push(name);
    }
    for (address, names) in &by_address {
        if names.len() > 1 {
            warnings.push(format!("0x{address:08X} appears several times: {}", names.join(", ")));
        }
    }

    if let Some(runtime) = option(args, "--runtime") {
        let call_list = Path::new(&runtime).join("ps2xRuntime").join("include").join("ps2_call_list.h");
        let handlers = call_list_handlers(&std::fs::read_to_string(&call_list).unwrap_or_default());
        for (name, address) in &stubs {
            if !handlers.contains(name) && !handlers.iter().any(|h| h == name.trim_start_matches('_')) {
                errors.push(format!("stubs: {name}@0x{address:08X} has no handler in ps2_call_list.h"));
            }
        }
    }

    check_iop_manifest(&root.join(GAME_PROFILE).join("iop.manifest"), &mut errors);

    for warning in &warnings {
        println!("::warning::{warning}");
    }
    for error in &errors {
        println!("::error::{error}");
    }
    println!(
        "funcmap.csv: {} functions; stubs: {}; entry_points: {}; errors: {}; warnings: {}",
        functions.len(),
        stubs.len(),
        entries.len(),
        errors.len(),
        warnings.len()
    );
    i32::from(!errors.is_empty())
}

// --- validate-tests -----------------------------------------------------------------------------------------------

/// `text` with every ANSI color code replaced by a NUL, which delimits names like the codes did.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            let mut probe = chars.clone();
            probe.next();
            while matches!(probe.peek(), Some(d) if d.is_ascii_digit() || *d == ';') {
                probe.next();
            }
            if probe.peek() == Some(&'m') {
                probe.next();
                chars = probe;
                out.push('\0');
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// `{"suite :: test": "Passed" | "Failed" | "Error"}` from MiniTest's output (a name ends at the first line break or
/// color code; the test prints whatever it wants before its `[Passed]`/`[Failed]`/`[Error]`).
pub fn results(output: &str) -> BTreeMap<String, String> {
    const RUN: &str = "  [Run]: ";
    let text = strip_ansi(output);
    let mut suites: Vec<(usize, String)> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        if let Some(name) = content.strip_prefix("[Suite]: ") {
            suites.push((offset, name.replace('\0', "").trim().to_owned()));
        }
        offset += line.len();
    }
    let mut chunks = text.split(RUN);
    let mut position = chunks.next().map_or(0, str::len);
    let mut results = BTreeMap::new();
    for chunk in chunks {
        let start = position;
        position += RUN.len() + chunk.len();
        let suite = suites.iter().rev().find(|(at, _)| *at < start).map_or("?", |(_, name)| name.as_str());
        let name = chunk.trim_start_matches('\0').split(['\n', '\0']).next().unwrap_or("").trim();
        let cleaned = chunk.replace('\0', "");
        let status = ["[Passed]", "[Failed]", "[Error]"]
            .iter()
            .filter_map(|marker| cleaned.find(marker).map(|at| (at, *marker)))
            .min_by_key(|(at, _)| *at)
            .map_or("Error", |(_, marker)| &marker[1..marker.len() - 1]);
        results.insert(format!("{suite} :: {name}"), status.to_owned());
    }
    results
}

/// What `validate-tests` decides about one run.
pub struct SuiteCheck {
    pub lines: Vec<String>,
    pub code: i32,
}

pub fn check_suite(output: &str, known_text: &str) -> SuiteCheck {
    let known: Vec<&str> = known_text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let res = results(output);
    let passing: Vec<&String> = res.iter().filter(|(_, e)| *e == "Passed").map(|(n, _)| n).collect();
    let failing: Vec<&String> = res.iter().filter(|(_, e)| *e != "Passed").map(|(n, _)| n).collect();
    let summary = output.contains("Total Tests:");

    let new: Vec<&&String> = failing.iter().filter(|n| !known.contains(&n.as_str())).collect();
    let fixed: Vec<&&String> = passing.iter().filter(|n| known.contains(&n.as_str())).collect();
    let known_failing = failing.iter().filter(|n| known.contains(&n.as_str())).count();

    let mut lines = vec![
        "\n==== Suite check ====".to_owned(),
        format!("Passing: {}  Failing: {}  Known failures: {}", passing.len(), failing.len(), known_failing),
    ];
    for name in fixed {
        lines.push(format!("::notice::Now passes (it can be removed from the known failures list): {name}"));
    }
    for name in &new {
        lines.push(format!("::error::New failure: {name}"));
    }
    if !summary {
        lines.push("::error::The suite did not print its summary: it hung or ended early".to_owned());
    }
    if passing.is_empty() && failing.is_empty() {
        lines.push("::error::No test result found in the output".to_owned());
        return SuiteCheck { lines, code: 1 };
    }
    SuiteCheck { lines, code: i32::from(!new.is_empty() || !summary) }
}

pub fn validate_tests(args: &[String]) -> i32 {
    let Some(known_path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: ps2_tests_output | ps2-dev validate-tests KNOWN_FAILURES.txt");
        return 2;
    };
    let known = std::fs::read_to_string(known_path).unwrap_or_default();
    let mut bytes = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes);
    let output = String::from_utf8_lossy(&bytes).into_owned();
    print!("{output}");
    let check = check_suite(&output, &known);
    for line in &check.lines {
        println!("{line}");
    }
    check.code
}
