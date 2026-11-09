//! `process::start` against a real child: the environment the game would see, the log, the exit report and `stop`.
//! One test function, because it sets process-wide environment variables.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gow_core::game::CRC_GOD_OF_WAR;
use gow_core::paths::AppPaths;
use gow_core::process::{self, GameFinished, StartOptions};
use gow_core::settings::{DisplayInfo, Settings};

fn runner() -> String {
    env!("CARGO_BIN_EXE_fake-runner").to_string()
}

/// An 8-byte ELF whose XOR of words is the CRC of the official game.
fn official_elf() -> Vec<u8> {
    let first = u32::from_le_bytes(*b"\x7FELF");
    let mut bytes = b"\x7FELF".to_vec();
    bytes.extend_from_slice(&(first ^ CRC_GOD_OF_WAR).to_le_bytes());
    bytes
}

fn run(settings: &Settings, paths: &AppPaths, options: &StartOptions) -> (GameFinished, String) {
    let (sender, receiver) = mpsc::channel();
    process::start(settings, options, paths, move |finished| {
        let _ = sender.send(finished);
    })
    .expect("the game starts");
    let finished = receiver.recv_timeout(Duration::from_secs(30)).expect("the game ends");
    let log = std::fs::read_to_string(paths.last_log()).unwrap_or_default();
    (finished, log)
}

fn value<'a>(log: &'a str, name: &str) -> Option<&'a str> {
    log.lines().find_map(|line| line.strip_prefix(&format!("ENV {name}=")))
}

#[test]
fn the_game_is_started_with_the_contracted_environment() {
    let directory = tempfile::tempdir().unwrap();
    let game = directory.path().join("game");
    std::fs::create_dir_all(&game).unwrap();
    let elf = game.join("SCUS_973.99");
    std::fs::write(&elf, official_elf()).unwrap();
    std::fs::write(game.join("b.iso"), b"x").unwrap();
    let paths = AppPaths::new(directory.path().join("data"), directory.path().join("cache"));

    let mut settings = Settings {
        elf: elf.to_string_lossy().into_owned(),
        executable: runner(),
        cheats: "god_mode,max_orbs".into(),
        cheats_shortcut: "9+11".into(),
        fast_boot: true,
        fxaa: false,
        filter: "fsr".into(),
        sharpness: 0.5,
        ..Settings::default()
    };
    settings.migrate(false, &DisplayInfo::default());
    // Migration 2 turns fast boot off; the user turned it on afterwards.
    settings.fast_boot = true;

    // The user's own variables are respected; the rest of the base environment is filled in.
    std::env::set_var("GOW_PRESENT_BLOCKING", "7");
    std::env::set_var("GOW_PERF_DIAG", "0");
    // These belong to the launcher's settings: an inherited value must not leak into a disabled option.
    std::env::set_var("GOW_SKIP_FMV", "1");
    std::env::set_var("PC_FXAA", "1");
    std::env::set_var("PC_RA_PASSWORD", "must-not-survive");

    let capture = directory.path().join("vu1").to_string_lossy().into_owned();
    let options = StartOptions { vu1_capture: capture.clone() };
    let (finished, log) = run(&settings, &paths, &options);

    assert_eq!(finished.code, Some(0));
    assert!(!finished.failed, "{finished:?}");
    assert!(log.contains(&format!("ARGS [{:?}]", settings.elf)), "{log}");
    assert!(log.contains(&format!("CWD {}", paths.data_dir().display())), "{log}");

    assert_eq!(value(&log, "PS2X_GS_WORKER"), Some("1"));
    assert_eq!(value(&log, "GOW_PRESENT_BLOCKING"), Some("7"), "the user's value wins");
    assert_eq!(value(&log, "GOW_PERF_DIAG"), Some("0"), "the user's value wins");
    assert_eq!(value(&log, "GOW_SHARED_WAIT_US"), Some("0"));
    assert_eq!(value(&log, "PS2X_GS_SCALE"), Some("2.0"));
    assert_eq!(value(&log, "GOW_SCALER"), Some("sharp"));
    assert_eq!(value(&log, "GOW_SHARPEN"), Some("0.25"));
    assert_eq!(value(&log, "GOW_FAST_BOOT"), Some("1"));
    assert_eq!(value(&log, "GOW_SKIP_FMV"), None, "videos are on after the migrations");
    assert_eq!(value(&log, "PC_FXAA"), None, "FXAA is off in the settings");
    assert_eq!(value(&log, "PC_CHEATS"), Some("god_mode,max_orbs"));
    assert_eq!(value(&log, "PC_CHEATS_SHORTCUT"), Some("9+11"));
    assert_eq!(
        value(&log, "GOW_INGAME_STATE"),
        Some(paths.ingame_state_file().to_str().unwrap()),
        "where the game writes its changes"
    );
    assert_eq!(value(&log, "PC_RA_ENABLED"), Some("0"));
    assert_eq!(value(&log, "PC_RA_PASSWORD"), None, "an inherited password is not passed on");
    assert!(value(&log, "PC_KEYS").unwrap().starts_with("ls_up=87;"));
    assert_eq!(value(&log, "GOW_VU1_CAPTURE"), Some(capture.as_str()));
    assert!(value(&log, "GOW_ISO").unwrap().ends_with("b.iso"), "the first ISO next to the ELF");
    assert!(Path::new(value(&log, "PS2X_GS_SHADER_CACHE_DIR").unwrap()).ends_with("shaders"));
    // The icon is written by the app at start-up; here it does not exist, so it must not be passed.
    assert_eq!(value(&log, "PC_ICON"), None);

    // What the player changed inside the game comes back into the settings once the game has ended, and the file is consumed.
    std::env::set_var("FAKE_RUNNER_INGAME", r#"{"cheats":"max_orbs","showFps":true,"brightness":70}"#);
    let (_, _) = run(&settings, &paths, &options);
    std::env::remove_var("FAKE_RUNNER_INGAME");
    assert!(paths.ingame_state_file().exists(), "the game wrote its state");
    let mut after = settings.clone();
    assert!(gow_core::ingame::consume(&mut after, &paths.ingame_state_file()));
    assert_eq!(after.cheats, "max_orbs");
    assert!(after.show_fps);
    assert_eq!(after.brightness, 70);
    assert_eq!(after.contrast, settings.contrast, "what the game did not send is left alone");
    assert!(!paths.ingame_state_file().exists(), "read once");

    // A second run rotates the log.
    let (_, second) = run(&settings, &paths, &options);
    assert!(!second.is_empty());
    assert!(paths.previous_log().exists(), "the previous run is kept");

    // A crash reports the exit code and the last error lines.
    std::env::set_var("FAKE_RUNNER_EXIT", "3");
    std::env::set_var("FAKE_RUNNER_MESSAGE", "fatal: the GS blew up");
    let (crashed, _) = run(&settings, &paths, &options);
    assert!(crashed.failed);
    assert_eq!(crashed.code, Some(3));
    assert!(crashed.summary.contains("fatal: the GS blew up"), "{}", crashed.summary);
    std::env::remove_var("FAKE_RUNNER_EXIT");
    std::env::remove_var("FAKE_RUNNER_MESSAGE");

    // Stopping ends a running game (a console process has no window to close, so it is killed after 3 s).
    std::env::set_var("FAKE_RUNNER_SLEEP_MS", "60000");
    let (sender, receiver) = mpsc::channel();
    let started = Instant::now();
    let session = process::start(&settings, &options, &paths, move |finished| {
        let _ = sender.send(finished);
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    session.stop();
    let stopped = receiver.recv_timeout(Duration::from_secs(20)).expect("the game was stopped");
    assert!(stopped.failed);
    assert!(started.elapsed() < Duration::from_secs(15));
    std::env::remove_var("FAKE_RUNNER_SLEEP_MS");

    // Refused before anything starts.
    settings.elf = directory.path().join("missing").to_string_lossy().into_owned();
    let (sender, _receiver) = mpsc::channel::<GameFinished>();
    let error = process::start(&settings, &options, &paths, move |f| {
        let _ = sender.send(f);
    })
    .unwrap_err();
    assert_eq!(error.key(), "errorElf");
}
