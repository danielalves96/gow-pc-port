//! Prints its arguments, working directory and the environment variables the port cares about, then exits with the
//! code in `FAKE_RUNNER_EXIT` (default 0). `FAKE_RUNNER_MESSAGE` goes to stderr and `FAKE_RUNNER_SLEEP_MS` makes it wait.

use std::time::Duration;

const PREFIXES: [&str; 6] = ["PC_", "GOW_", "PS2X_", "DRI_PRIME", "__NV_", "__GLX_"];

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    println!("ARGS {arguments:?}");
    println!("CWD {}", std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default());
    let mut names: Vec<(String, String)> =
        std::env::vars().filter(|(name, _)| PREFIXES.iter().any(|p| name.starts_with(p))).collect();
    names.sort();
    for (name, value) in names {
        println!("ENV {name}={value}");
    }
    if let Ok(message) = std::env::var("FAKE_RUNNER_MESSAGE") {
        eprintln!("{message}");
    }
    // what the game writes when the player changes something inside it (`FAKE_RUNNER_INGAME` is the JSON to write)
    if let (Ok(json), Ok(path)) = (std::env::var("FAKE_RUNNER_INGAME"), std::env::var("GOW_INGAME_STATE")) {
        let _ = std::fs::write(path, json);
    }
    if let Some(ms) = std::env::var("FAKE_RUNNER_SLEEP_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
        std::thread::sleep(Duration::from_millis(ms));
    }
    std::process::exit(std::env::var("FAKE_RUNNER_EXIT").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
}
