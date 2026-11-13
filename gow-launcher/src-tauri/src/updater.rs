//! Self-update glue: finds a newer launcher release (`gow_core::update`), asks `tauri-plugin-updater` to verify, download and
//! install it, and reports every step to the interface as an `update-state` event. What can be decided without Tauri
//! (which release is newer, how this copy was installed, what blocks an update) is in `gow-core`.
//!
//! A failed check is never shown: it goes to `logs/update.log` and the next try comes later (see `RETRY_INTERVAL`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gow_core::shortcut;
use gow_core::update::{self, Activity, Mode, Phase, UpdateState};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use super::AppState;

/// The checks do not run when this variable is set (tests of the launcher, or someone who does not want them).
const DISABLE_VARIABLE: &str = "GOW_LAUNCHER_NO_UPDATE_CHECK";
/// Wait after the window opens, so the first request does not compete with the start-up work.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(8);
/// How often the scheduler looks at what changed (a release found, the game closed...), not how often it asks GitHub.
const TICK: Duration = Duration::from_secs(20);
const PROGRESS_EVERY: Duration = Duration::from_millis(120);
/// Waits before the second and third download attempts. A download that dies halfway ("error decoding response body", a
/// 504 from GitHub's file host) usually works on the next try; players saw it fail up to three times in a row before.
const DOWNLOAD_RETRY_WAITS: [Duration; 2] = [Duration::from_secs(3), Duration::from_secs(10)];

/// A network failure is worth another attempt; a bad signature or package is not.
fn worth_retrying(error: &tauri_plugin_updater::Error) -> bool {
    matches!(error, tauri_plugin_updater::Error::Reqwest(_) | tauri_plugin_updater::Error::Network(_))
}

/// The release found by the check, and its installer once downloaded (kept when the install had to wait).
struct Pending {
    update: Update,
    bytes: Option<Vec<u8>>,
}

pub struct Updates {
    state: Mutex<UpdateState>,
    pending: Mutex<Option<Pending>>,
    /// A download or an installation is running.
    working: AtomicBool,
    next_check: Mutex<Option<Instant>>,
    log: PathBuf,
}

impl Updates {
    pub fn new(log: PathBuf) -> Self {
        Self {
            state: Mutex::new(UpdateState::default()),
            pending: Mutex::new(None),
            working: AtomicBool::new(false),
            next_check: Mutex::new(None),
            log,
        }
    }

    pub fn snapshot(&self) -> UpdateState {
        self.state.lock().unwrap().clone()
    }

    fn log(&self, message: &str) {
        update::log(&self.log, message);
    }

    /// Changes the state and tells the interface.
    fn set(&self, app: &AppHandle, change: impl FnOnce(&mut UpdateState)) {
        let snapshot = {
            let mut state = self.state.lock().unwrap();
            change(&mut state);
            state.clone()
        };
        let _ = app.emit("update-state", snapshot);
    }
}

fn activity(state: &AppState) -> Activity {
    Activity {
        game: state.game.lock().unwrap().is_some(),
        installation: state.installation.lock().unwrap().is_some(),
        optimizer: state.optimizer.snapshot().compiling,
        texture_pack: state.textures_running.load(Ordering::SeqCst),
    }
}

/// `plugins.updater.pubkey` of `tauri.conf.json`: the placeholder until the maintainer sets the real key.
fn configured_pubkey(app: &AppHandle) -> String {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(|key| key.as_str())
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------------------------- checking

/// What the signed manifest of the release says about this platform (none: the release has nothing for this system).
fn resolve(app: &AppHandle, manifest_url: &str) -> Result<Option<Update>, String> {
    if !update::trusted_url(manifest_url) {
        return Err(format!("refusing to read the manifest from {manifest_url}"));
    }
    let url: tauri::Url = manifest_url.parse().map_err(|error| format!("bad manifest address: {error}"))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .and_then(|builder| builder.timeout(Duration::from_secs(20)).build())
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::block_on(updater.check()).map_err(|error| error.to_string())
}

/// One round: GitHub's release list, then (when this copy can update itself) the signed manifest. True when the question
/// could be answered, whatever the answer was.
fn check(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let updates = &state.updater;
    if updates.working.load(Ordering::SeqCst) {
        return true;
    }
    let release = match update::latest_update(&update::releases_url(), env!("CARGO_PKG_VERSION")) {
        Ok(Some(release)) => release,
        Ok(None) => {
            *updates.pending.lock().unwrap() = None;
            updates.set(app, |state| *state = UpdateState::default());
            return true;
        }
        Err(error) => {
            updates.log(&format!("check failed: {error}"));
            return false;
        }
    };

    let kind = update::install_kind();
    let key_usable = update::pubkey_usable(&configured_pubkey(app));
    let mut mode = update::mode_for(kind, release.manifest_url.is_some(), key_usable);
    let mut version = release.version.to_string();
    if kind == update::InstallKind::Installed && mode == Mode::Manual {
        let reason = if release.manifest_url.is_none() {
            "the release has no latest.json"
        } else {
            "no usable updater public key"
        };
        updates.log(&format!("{version} is available but cannot be installed in place: {reason}"));
    }

    let mut found = None;
    if let (Mode::Automatic, Some(manifest)) = (mode, release.manifest_url.as_deref()) {
        match resolve(app, manifest) {
            Ok(Some(update)) => {
                version = update.version.clone();
                found = Some(update);
            }
            Ok(None) => {
                updates.log(&format!("the manifest of {version} offers nothing for this system"));
                mode = Mode::Manual;
            }
            Err(error) => {
                updates.log(&format!("the manifest of {version} could not be used: {error}"));
                mode = Mode::Manual;
            }
        }
    }

    {
        let mut pending = updates.pending.lock().unwrap();
        match found {
            // The same version again keeps what was already downloaded.
            Some(update) if pending.as_ref().is_some_and(|p| p.update.version == update.version) => {
                let _ = update;
            }
            Some(update) => *pending = Some(Pending { update, bytes: None }),
            None => *pending = None,
        }
    }
    updates.log(&format!("{version} is available (mode {mode:?})"));
    updates.set(app, |state| {
        // A failure of this very version stays on screen until the player retries.
        if state.phase == Phase::Error && state.version == version {
            state.mode = mode;
            return;
        }
        *state = UpdateState {
            phase: Phase::Available,
            mode,
            version,
            page_url: release.page_url,
            ..UpdateState::default()
        };
    });
    true
}

// ---------------------------------------------------------------------------------------------- installing

/// Starts the download and the installation in the background. Errors are the reasons it did not start; the rest is
/// reported through `update-state`.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let updates = &state.updater;
    let snapshot = updates.snapshot();
    if snapshot.mode != Mode::Automatic || !matches!(snapshot.phase, Phase::Available | Phase::Error) {
        return Err("notAvailable".into());
    }
    if updates.pending.lock().unwrap().is_none() {
        return Err("notAvailable".into());
    }
    if let Some(blocker) = activity(&state).blocker() {
        return Err(format!("busy:{}", blocker.code()));
    }
    if updates.working.swap(true, Ordering::SeqCst) {
        return Err("busy:update".into());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        run_install(&app);
        app.state::<AppState>().updater.working.store(false, Ordering::SeqCst);
    });
    Ok(())
}

fn run_install(app: &AppHandle) {
    let state = app.state::<AppState>();
    let updates = &state.updater;
    let Some(Pending { update, bytes }) = updates.pending.lock().unwrap().take() else { return };
    let fail = |update: Update, bytes: Option<Vec<u8>>, error: String| {
        updates.log(&format!("update to {} failed: {error}", update.version));
        *updates.pending.lock().unwrap() = Some(Pending { update, bytes });
        updates.set(app, |state| {
            state.phase = Phase::Error;
            state.error = error;
        });
    };

    let bytes = match bytes {
        Some(bytes) => bytes,
        None => {
            let mut attempt = 0usize;
            loop {
                updates.set(app, |state| {
                    state.phase = Phase::Downloading;
                    state.done = 0;
                    state.total = 0;
                    state.error.clear();
                });
                let (mut done, mut last_report) = (0u64, Instant::now());
                let downloaded = tauri::async_runtime::block_on(update.download(
                    |chunk, total| {
                        done += chunk as u64;
                        if last_report.elapsed() >= PROGRESS_EVERY {
                            last_report = Instant::now();
                            updates.set(app, |state| {
                                state.done = done;
                                state.total = total.unwrap_or(0);
                            });
                        }
                    },
                    || {},
                ));
                match downloaded {
                    Ok(bytes) => break bytes,
                    Err(error) => match DOWNLOAD_RETRY_WAITS.get(attempt) {
                        Some(wait) if worth_retrying(&error) => {
                            updates.log(&format!(
                                "download of {} failed ({error}); trying again in {} s",
                                update.version,
                                wait.as_secs()
                            ));
                            std::thread::sleep(*wait);
                            attempt += 1;
                        }
                        _ => return fail(update, None, error.to_string()),
                    },
                }
            }
        }
    };

    // The game, an installation or an optimization may have started while the file came down: it waits, already
    // downloaded, for the next moment that is safe.
    if let Some(blocker) = activity(&state).blocker() {
        updates.log(&format!("update to {} downloaded; waiting for {}", update.version, blocker.code()));
        *updates.pending.lock().unwrap() = Some(Pending { update, bytes: Some(bytes) });
        updates.set(app, |state| {
            state.phase = Phase::Available;
            state.error.clear();
        });
        return;
    }

    updates.set(app, |state| {
        state.phase = Phase::Installing;
        state.done = state.total;
    });
    updates.log(&format!("installing {}", update.version));
    // On Windows the installer takes over and this process exits inside `install`; elsewhere the new file is in place and
    // the launcher restarts into it.
    match update.install(&bytes) {
        Ok(()) => app.restart(),
        Err(error) => fail(update, None, error.to_string()),
    }
}

/// With "auto-update" on, installs a found update as soon as it is safe. An update that failed is not retried by itself:
/// the player sees the error and the button.
pub fn evaluate_auto(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.snapshot().auto_update {
        return;
    }
    let current = state.updater.snapshot();
    if current.phase == Phase::Available && current.mode == Mode::Automatic {
        let _ = install(app);
    }
}

/// Runs `evaluate_auto` now, off the caller's thread (the switch was just turned on).
pub fn nudge(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || evaluate_auto(&app));
}

/// Opens the release page in the browser (the notice's button in a portable copy or a package install).
pub fn open_release(app: &AppHandle) -> bool {
    let url = app.state::<AppState>().updater.snapshot().page_url;
    update::trusted_url(&url) && shortcut::open_path(std::path::Path::new(&url)).is_ok()
}

pub fn spawn_scheduler(app: AppHandle) {
    if std::env::var_os(DISABLE_VARIABLE).is_some_and(|value| !value.is_empty()) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            let state = app.state::<AppState>();
            let due = update::check_due(*state.updater.next_check.lock().unwrap(), Instant::now());
            if due && !state.updater.working.load(Ordering::SeqCst) {
                let answered = check(&app);
                let wait = if answered { update::CHECK_INTERVAL } else { update::RETRY_INTERVAL };
                *state.updater.next_check.lock().unwrap() = Some(Instant::now() + wait);
            }
            evaluate_auto(&app);
            std::thread::sleep(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin reads this section when the app starts (a mistake would stop the launcher from opening), so it is parsed
    /// here exactly as the plugin does.
    #[test]
    fn the_updater_section_of_the_config_is_one_the_plugin_accepts() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let section = config["plugins"]["updater"].clone();
        let parsed: tauri_plugin_updater::Config =
            serde_json::from_value(section).expect("plugins.updater must deserialize");
        assert!(!parsed.endpoints.is_empty());
        assert!(parsed.endpoints.iter().all(|url| url.scheme() == "https" && update::trusted_url(url.as_str())));
        // Either the documented placeholder (the feature stays off) or a real minisign public key; never something in between.
        assert!(
            parsed.pubkey == update::PUBKEY_PLACEHOLDER || update::pubkey_usable(&parsed.pubkey),
            "{}",
            parsed.pubkey
        );
    }

    #[test]
    fn the_release_page_opens_only_on_github() {
        assert!(update::trusted_url("https://github.com/danielalves96/gow-pc-port/releases/tag/launcher-v0.2.0"));
        assert!(!update::trusted_url("https://example.com/releases"));
    }

    #[test]
    fn only_network_failures_are_downloaded_again() {
        use tauri_plugin_updater::Error;
        assert!(worth_retrying(&Error::Network("Download request failed with status: 504 Gateway Timeout".into())));
        assert!(!worth_retrying(&Error::ReleaseNotFound));
        assert!(!worth_retrying(&Error::SignatureUtf8("not base64".into())));
    }
}
