//! `ps2-tools <command> ...`: see the usage text below.

use std::path::PathBuf;
use std::process::ExitCode;

use ps2_tools::{chd, elfcheck, iop_manifest, iso, memcard, vu1, xdelta};

const USAGE: &str = "usage:
  ps2-tools verify-elf ELF [--map funcmap.csv] [--allow-modified]
  ps2-tools iso-extract ISO [OUTDIR] [--list] [--only NAME,NAME] [--layout]
  ps2-tools vu1-manifest apply ELF MANIFEST OUTDIR
  ps2-tools vu1-manifest verify ELF MANIFEST CAPTURES
  ps2-tools vu1-manifest extend ELF MANIFEST CAPTURES OUT
  ps2-tools vu1-programs apply ELF MANIFEST OUTDIR
  ps2-tools vu1-programs report ELF MANIFEST
  ps2-tools iop-manifest apply GAMEDIR MANIFEST OUTDIR
  ps2-tools iop-manifest verify GAMEDIR MANIFEST DUMPDIR
  ps2-tools iop-gen DUMPDIR OUTDIR [--overrides FILE]
  ps2-tools vu1n-gen OUTDIR INPUT... [--heads heads.txt] [--parts N] [--cache FILE | --no-cache]
  ps2-tools xdelta-apply PATCH BASE_ISO OUTPUT_ISO
  ps2-tools chd-info CHD
  ps2-tools chd-extract CHD OUTPUT_ISO
  ps2-tools mc-format CARD.ps2
  ps2-tools mc-check CARD.ps2";

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|index| args.get(index + 1)).cloned()
}

/// The arguments that are neither a `--flag` nor the value of `--map`/`--only`.
fn positionals(args: &[String]) -> Vec<&String> {
    let mut result = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
        } else if arg == "--map" || arg == "--only" {
            skip = true;
        } else if !arg.starts_with("--") {
            result.push(arg);
        }
    }
    result
}

fn verify_elf(args: &[String]) -> i32 {
    let positional = positionals(args);
    let Some(elf) = positional.first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let map = option(args, "--map")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("RustPortPS2/games/scus-97399/funcmap.csv"));
    let report = match elfcheck::inspect(&PathBuf::from(elf), &map) {
        Ok(report) => report,
        Err(message) => {
            eprintln!("[gow-elf] {message}");
            return 1;
        }
    };
    println!("{}", report.to_json());
    // GOW-Port (decision D4): a translation with the USA code installs like the original; it is named, not refused.
    if !report.known_edition() && report.same_code() {
        println!("[gow-elf] Translated or re-released disc: the code is the USA one (same .text), so it is supported.");
        return 0;
    }
    if !report.known_edition() {
        let warning = "[gow-elf] Modified ELF: the map fits, but its compatibility is NOT verified.";
        if !flag(args, "--allow-modified") {
            eprintln!("{warning} Use --allow-modified only for experimental tests.");
            return 2;
        }
        println!("{warning}");
    }
    0
}

fn iso_extract(args: &[String]) -> i32 {
    let positional = positionals(args);
    let Some(iso_path) = positional.first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let options = iso::Options {
        iso: PathBuf::from(iso_path),
        output: positional.get(1).map(PathBuf::from),
        list: flag(args, "--list"),
        only: option(args, "--only").map(|names| names.to_uppercase().split(',').map(str::to_owned).collect()),
        layout: flag(args, "--layout"),
    };
    match iso::run(&options) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[iso-extract] {error}");
            1
        }
    }
}

/// GOW-Port (T5.4): progress every 256 MiB, then the result; the output appears only when every window checked out.
fn xdelta_apply(patch: &str, base: &str, output: &str) -> i32 {
    let mut next_report = 256u64 << 20;
    let progress = |bytes: u64| {
        if bytes >= next_report {
            println!("[xdelta] {} MiB", bytes >> 20);
            next_report = bytes + (256 << 20);
        }
    };
    match xdelta::apply(&PathBuf::from(patch), &PathBuf::from(base), &PathBuf::from(output), progress) {
        Ok(summary) => {
            println!("[xdelta] done: {} windows, {} bytes written to {output}", summary.windows, summary.bytes);
            0
        }
        Err(error) => {
            eprintln!("[xdelta] {error}");
            1
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn chd_info(path: &str) -> i32 {
    match chd::info(&PathBuf::from(path)) {
        Ok(info) => {
            println!("[chd] {path}");
            println!("[chd] version {}, {} image, codecs {}", info.version, info.kind(), info.codecs.join(", "));
            println!(
                "[chd] {} raw bytes in {} hunks of {} bytes; ISO {} bytes",
                info.logical_bytes, info.hunks, info.hunk_bytes, info.iso_bytes
            );
            if let chd::Layout::Cd { tracks } = &info.layout {
                for track in tracks {
                    println!("[chd] track {}: {}, {} frames", track.number, track.kind, track.frames);
                }
            }
            println!("[chd] raw SHA-1 {}", hex(&info.raw_sha1));
            0
        }
        Err(error) => {
            eprintln!("[chd] {error}");
            1
        }
    }
}

fn chd_extract(path: &str, output: &str) -> i32 {
    let mut next_report = 0u64;
    let started = std::time::Instant::now();
    let progress = |done: u64, total: u64| {
        if done >= next_report || done == total {
            println!("[chd] {} / {} MiB", done >> 20, total >> 20);
            next_report = done + (512 << 20);
        }
        true
    };
    match chd::extract_iso(&PathBuf::from(path), &PathBuf::from(output), 0, progress) {
        Ok(summary) => {
            println!(
                "[chd] done: {} bytes written to {output} in {:.0} s, every hunk and the SHA-1 checked",
                summary.iso_bytes,
                started.elapsed().as_secs_f64()
            );
            0
        }
        Err(error) => {
            eprintln!("[chd] {error}");
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let Some((command, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    // GOW-Port: `--help` anywhere prints the usage; it used to be taken as an output folder (vu1n-gen created "--help/").
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(0);
    }
    match (command.as_str(), rest) {
        ("verify-elf", rest) => Ok(verify_elf(rest)),
        ("iso-extract", rest) => Ok(iso_extract(rest)),
        ("xdelta-apply", [patch, base, output]) => Ok(xdelta_apply(patch, base, output)),
        ("chd-info", [path]) => Ok(chd_info(path)),
        ("chd-extract", [path, output]) => Ok(chd_extract(path, output)),
        ("mc-format", [card]) => {
            memcard::format_file(&PathBuf::from(card))?;
            println!("[mc-format] formatted 8 MB card written to {card}");
            Ok(0)
        }
        ("mc-check", [card]) => {
            let bytes = std::fs::read(card).map_err(|error| format!("{card}: {error}"))?;
            match memcard::check(&bytes) {
                Ok(report) => {
                    let state = if report.formatted { "formatted" } else { "blank (the game can format it)" };
                    println!("[mc-check] {card}: {state}, {} page(s) with a bad ECC", report.ecc_errors.len());
                    Ok(if report.ecc_errors.is_empty() { 0 } else { 1 })
                }
                Err(error) => {
                    eprintln!("[mc-check] {card}: {error}");
                    Ok(1)
                }
            }
        }
        ("iop-gen", [dump, output, rest @ ..]) => {
            let overrides = match option(rest, "--overrides") {
                Some(path) => std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?,
                None => ps2_tools::iop_gen::DEFAULT_OVERRIDES.to_owned(),
            };
            ps2_tools::iop_gen::run(&PathBuf::from(dump), &PathBuf::from(output), &overrides)?;
            Ok(0)
        }
        ("vu1n-gen", [output, rest @ ..]) => {
            let mut inputs = Vec::new();
            let (mut heads, mut parts) = (None, 6usize);
            // GOW-Port: incremental cache in the output folder unless --no-cache (or --cache <file> elsewhere).
            let mut cache = Some(PathBuf::from(output).join(".vu1n-cache"));
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--heads" => {
                        heads = Some(PathBuf::from(rest.get(index + 1).ok_or(USAGE)?));
                        index += 2;
                    }
                    "--no-cache" => {
                        cache = None;
                        index += 1;
                    }
                    "--cache" => {
                        cache = Some(PathBuf::from(rest.get(index + 1).ok_or(USAGE)?));
                        index += 2;
                    }
                    "--parts" => {
                        parts = rest.get(index + 1).and_then(|n| n.parse().ok()).ok_or(USAGE)?;
                        index += 2;
                    }
                    other => {
                        inputs.push(PathBuf::from(other));
                        index += 1;
                    }
                }
            }
            let options = ps2_tools::vu1n_gen::Options { output: PathBuf::from(output), inputs, heads, parts, cache };
            println!("{}", ps2_tools::vu1n_gen::run(&options)?);
            Ok(0)
        }
        ("vu1-manifest", [sub, elf, manifest, captures]) => match sub.as_str() {
            "apply" => vu1::apply_manifest(&PathBuf::from(elf), &PathBuf::from(manifest), &PathBuf::from(captures)),
            "verify" => vu1::verify_manifest(&PathBuf::from(elf), &PathBuf::from(manifest), &PathBuf::from(captures)),
            _ => Err(USAGE.to_owned()),
        },
        ("vu1-manifest", [sub, elf, manifest, captures, output]) if sub == "extend" => vu1::extend_manifest(
            &PathBuf::from(elf),
            &PathBuf::from(manifest),
            &PathBuf::from(captures),
            &PathBuf::from(output),
        ),
        ("iop-manifest", [sub, game, manifest, folder]) => {
            let (game, manifest, folder) = (PathBuf::from(game), PathBuf::from(manifest), PathBuf::from(folder));
            match sub.as_str() {
                "apply" => iop_manifest::apply_manifest(&game, &manifest, &folder),
                "verify" => iop_manifest::verify_manifest(&game, &manifest, &folder),
                _ => Err(USAGE.to_owned()),
            }
        }
        ("vu1-programs", [sub, elf, manifest, rest @ ..]) => match (sub.as_str(), rest) {
            ("apply", [folder]) => {
                vu1::apply_programs(&PathBuf::from(elf), &PathBuf::from(manifest), &PathBuf::from(folder))
            }
            ("report", []) => vu1::report_programs(&PathBuf::from(elf), &PathBuf::from(manifest)),
            _ => Err(USAGE.to_owned()),
        },
        _ => Err(USAGE.to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
