//! Starting, watching and stopping the game.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::credential;
use crate::failure;
use crate::gpu;
use crate::launch::{self, LaunchContext, LaunchError};
use crate::paths::AppPaths;
use crate::settings::Settings;

/// How a game session ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFinished {
    pub code: Option<i32>,
    pub failed: bool,
    pub seconds: u64,
    pub summary: String,
    /// The interface string that explains a recognized failure (see [`crate::failure::FailureKind`]).
    pub failure_kind: Option<String>,
}

/// A running game. Dropping it does not stop the game: it stays open if the launcher is closed.
#[derive(Debug, Clone, Copy)]
pub struct GameSession {
    pid: u32,
}

impl GameSession {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Asks the game to close and kills it after three seconds. Only the PID this launcher opened is touched.
    pub fn stop(&self) {
        let pid = self.pid;
        terminate(pid);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            kill(pid);
        });
    }
}

/// Inputs of [`start`] that the interface knows and the settings do not.
#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    /// Folder where the optimizer wants the new VU1 microprograms recorded (empty = off).
    pub vu1_capture: String,
}

/// Rotates the logs and picks a file the game can write to: the usual `last.log`, or an alternative name, or the temp
/// folder. Returns the path and the opened file.
fn open_log(paths: &AppPaths) -> Result<(PathBuf, File), LaunchError> {
    let logs = paths.logs_dir();
    let _ = fs::create_dir_all(&logs);
    let last = paths.last_log();
    let _ = fs::remove_file(paths.previous_log());
    let _ = fs::rename(&last, paths.previous_log());
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let candidates = [last, logs.join(format!("last-{seconds}.log")), std::env::temp_dir().join("PC-last.log")];
    let mut reason = String::new();
    for candidate in candidates {
        match OpenOptions::new().create(true).append(true).open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if reason.is_empty() => reason = format!("{}: {error}", candidate.display()),
            Err(_) => {}
        }
    }
    Err(LaunchError::LogOpen(reason))
}

/// Starts the runner with the environment the settings produce. `on_finished` runs on a watcher thread.
pub fn start(
    settings: &Settings,
    options: &StartOptions,
    paths: &AppPaths,
    on_finished: impl FnOnce(GameFinished) + Send + 'static,
) -> Result<GameSession, LaunchError> {
    launch::validate(settings)?;

    let cards = gpu::graphics_cards();
    let wanted = if settings.gpu == "auto" || cards.iter().any(|card| card.id == settings.gpu) {
        settings.gpu.clone()
    } else {
        "auto".to_string()
    };
    let ctx = LaunchContext {
        settings,
        scale: settings.selected_scale(),
        vu1_capture: options.vu1_capture.clone(),
        gpu: wanted.clone(),
        icon: Some(paths.icon()),
        shader_cache: paths.shader_cache(),
        pad_devices_file: Some(paths.pad_devices_file()),
        retro_achievements_password: settings
            .retro_achievements_enabled
            .then(|| credential::retro_achievements_password(&settings.retro_achievements_user))
            .flatten(),
        refresh_hz: crate::display::refresh_rate(),
    };
    #[cfg_attr(windows, allow(unused_mut))]
    let mut plan = launch::build_environment(&ctx, |name| std::env::var(name).ok());
    let _ = fs::create_dir_all(paths.shader_cache());
    #[cfg(windows)]
    {
        let _ = gpu::apply_windows_preference(&wanted, &settings.executable);
    }
    #[cfg(not(windows))]
    gpu::linux_selection(&cards, &wanted, &mut plan);

    let (log_path, log) = open_log(paths)?;
    let _ = fs::create_dir_all(paths.data_dir());
    let mut command = Command::new(&settings.executable);
    command
        .arg(&settings.elf)
        .current_dir(paths.data_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().map_err(|e| LaunchError::LogOpen(e.to_string()))?))
        .stderr(Stdio::from(log));
    crate::system::sanitize_child_environment(&mut command);
    for name in &plan.remove {
        command.env_remove(name);
    }
    for (name, value) in &plan.set {
        command.env(name, value);
    }
    // Where the game writes what the player changes inside it (the launcher reads it when the game ends, `ingame.rs`).
    command.env("GOW_INGAME_STATE", paths.ingame_state_file());
    let mut child = command.spawn().map_err(|error| LaunchError::LogOpen(error.to_string()))?;
    let session = GameSession { pid: child.id() };
    let started = Instant::now();
    std::thread::spawn(move || {
        let status = child.wait();
        let seconds = started.elapsed().as_secs();
        let (code, crashed) = match status {
            Ok(status) => (status.code(), status.code().is_none()),
            Err(_) => (None, true),
        };
        let failed = crashed || code != Some(0);
        let (summary, failure_kind) = if failed {
            let text =
                fs::read(&log_path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
            let kind = failure::classify_game_failure(&text, code).map(|kind| kind.key().to_string());
            (summarize(&text), kind)
        } else {
            (String::new(), None)
        };
        on_finished(GameFinished { code, failed, seconds, summary, failure_kind });
    });
    Ok(session)
}

/// Last error lines of the log for the notice: lines mentioning error/fatal/failed, or else the last four.
pub fn failure_summary(log: &Path) -> String {
    let Ok(bytes) = fs::read(log) else {
        return String::new();
    };
    summarize(&String::from_utf8_lossy(&bytes))
}

pub fn summarize(text: &str) -> String {
    let lines: Vec<&str> =
        text.split('\n').map(|line| line.trim_end_matches('\r')).filter(|line| !line.is_empty()).collect();
    let cut = |line: &str| line.chars().take(200).collect::<String>();
    let mut relevant: Vec<String> = Vec::new();
    for line in lines.iter().rev() {
        if relevant.len() >= 6 {
            break;
        }
        let lower = line.to_lowercase();
        if lower.contains("error") || lower.contains("fatal") || lower.contains("failed") {
            relevant.push(cut(line));
        }
    }
    relevant.reverse();
    if relevant.is_empty() {
        relevant = lines[lines.len().saturating_sub(4)..].iter().map(|line| cut(line)).collect();
    }
    relevant.join("\n")
}

#[cfg(windows)]
fn terminate(pid: u32) {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE};

    unsafe extern "system" fn close_window(window: HWND, target: LPARAM) -> BOOL {
        let mut owner = 0u32;
        // SAFETY: `window` is a valid handle handed to us by EnumWindows.
        unsafe { GetWindowThreadProcessId(window, &mut owner) };
        if owner == target as u32 {
            // SAFETY: posting a message to a window we just enumerated.
            unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        }
        TRUE
    }
    // SAFETY: the callback only reads its arguments.
    unsafe { EnumWindows(Some(close_window), pid as LPARAM) };
}

#[cfg(windows)]
fn kill(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    // SAFETY: the handle is opened, used once and closed here.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

#[cfg(unix)]
fn terminate(pid: u32) {
    // SAFETY: plain signal to the PID this launcher spawned.
    unsafe { libc::kill(pid as i32, libc::SIGTERM) };
}

#[cfg(unix)]
fn kill(pid: u32) {
    // SAFETY: as above; ESRCH if it already exited is harmless.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_prefers_error_lines_and_keeps_six() {
        let mut log = String::new();
        for index in 0..10 {
            log.push_str(&format!("ok line {index}\nERROR number {index}\n"));
        }
        let summary = summarize(&log);
        let lines: Vec<&str> = summary.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines.first(), Some(&"ERROR number 4"));
        assert_eq!(lines.last(), Some(&"ERROR number 9"));
    }

    #[test]
    fn summary_falls_back_to_the_last_four_lines_truncated() {
        let long = "x".repeat(300);
        let summary = summarize(&format!("a\nb\nc\nd\ne\n{long}\n"));
        let lines: Vec<&str> = summary.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "c");
        assert_eq!(lines[3].chars().count(), 200);
        assert_eq!(summarize(""), "");
    }

    #[test]
    fn the_log_is_rotated() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(directory.path().join("data"), directory.path().join("cache"));
        fs::create_dir_all(paths.logs_dir()).unwrap();
        fs::write(paths.last_log(), "old run").unwrap();
        let (path, _file) = open_log(&paths).unwrap();
        assert_eq!(path, paths.last_log());
        assert_eq!(fs::read_to_string(paths.previous_log()).unwrap(), "old run");
        assert_eq!(fs::read_to_string(paths.last_log()).unwrap(), "");
    }
}
