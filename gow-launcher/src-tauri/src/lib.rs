//! Tauri shell of the launcher: commands and events over `gow-core`. No launcher logic lives here.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gow_core::controller::{self, PadDeviceList};
use gow_core::credential;
use gow_core::failure::FailureKind;
use gow_core::game::{self, DataState, DetectedPaths, ExecutableState, GameState};
use gow_core::gamepad::{self, Capture, CaptureResult};
use gow_core::gpu::{self, GraphicsCard};
use gow_core::installer::{self, InstallRequest, Installation};
use gow_core::iso::{self, IsoValidation};
use gow_core::iso_chd::{self, ChdOutcome, ChdPlan};
use gow_core::iso_patch::{self, PatchError, PatchOutcome};
use gow_core::launch::LaunchError;
use gow_core::optimizer::{self, Optimizer, OptimizerSnapshot};
use gow_core::paths::AppPaths;
use gow_core::process::{self, GameFinished, GameSession, StartOptions};
use gow_core::report::{self, session, HostFacts, ReportRequest};
use gow_core::settings::{DisplayInfo, Settings};
use gow_core::update::Activity;
use gow_core::{direct, display, kit, legacy, rebuild, shortcut, textures};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

mod updater;

const ICON: &[u8] = include_bytes!("../../assets/icon-source.png");

struct AppState {
    paths: AppPaths,
    settings: Mutex<Settings>,
    game: Mutex<Option<GameSession>>,
    installation: Mutex<Option<Installation>>,
    optimizer: Optimizer,
    gamepad_cancel: Mutex<Option<Arc<AtomicBool>>>,
    searching: AtomicBool,
    textures_running: AtomicBool,
    /// GOW-Port (0.3.0): the translation patch being applied from the wizard, set to stop it.
    patch_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// GOW-Port (0.3.1): the CHD being converted to ISO from the wizard, set to stop it.
    chd_cancel: Mutex<Option<Arc<AtomicBool>>>,
    updater: updater::Updates,
    /// GOW-Port (issue #40): the launcher was started to open the game directly and has no window (yet).
    windowless: AtomicBool,
    /// The running game was started directly: closing a window opened meanwhile does not end the launcher before the game,
    /// so the session is still counted and Steam still sees the game running.
    started_directly: AtomicBool,
    /// A direct start was tried (the window must not press PLAY again for `--play`).
    direct_attempted: AtomicBool,
    /// Why a direct start ended in the window, for the interface to show once.
    notice: Mutex<Option<StartNotice>>,
}

/// What the window shows when it opens because a direct start did not go as planned.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartNotice {
    /// The game started and closed with an error.
    finished: Option<GameFinished>,
    /// The game could not start.
    launch_error: Option<CommandError>,
}

impl AppState {
    /// Applies `change` to the settings, writes the file and returns the new value.
    fn update(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let mut settings = self.settings.lock().unwrap();
        change(&mut settings);
        let _ = settings.save(&self.paths.settings_file());
        settings.clone()
    }

    /// Like `update`, but the file is written (and the UI told by the caller) only when `change` says it changed something.
    fn update_if(&self, change: impl FnOnce(&mut Settings) -> bool) {
        let mut settings = self.settings.lock().unwrap();
        if change(&mut settings) {
            let _ = settings.save(&self.paths.settings_file());
        }
    }

    fn snapshot(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
}

fn launcher_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)).unwrap_or_default()
}

fn install_script(state: &AppState) -> Option<PathBuf> {
    installer::find_install_script(&state.paths.kit_dir(), &launcher_dir())
}

fn display_info(window: &WebviewWindow) -> DisplayInfo {
    let size = window.primary_monitor().ok().flatten().map(|monitor| *monitor.size());
    let fallback = DisplayInfo::default();
    DisplayInfo {
        width_px: size.map_or(fallback.width_px, |s| s.width),
        height_px: size.map_or(fallback.height_px, |s| s.height),
        hz: display::refresh_rate(),
    }
}

// ---------------------------------------------------------------------------------------------- bootstrap

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchArguments {
    play: bool,
    wizard: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    settings: Settings,
    version: &'static str,
    platform: &'static str,
    display: DisplayInfo,
    graphics_cards: Vec<GraphicsCard>,
    data_directory: String,
    arguments: LaunchArguments,
    launcher_path: String,
    notice: Option<StartNotice>,
}

/// First call of the interface: migrates the settings (needs the system language and the display) and returns
/// everything it needs to draw.
#[tauri::command]
fn bootstrap(window: WebviewWindow, state: State<'_, AppState>, locale: String) -> Bootstrap {
    let display = display_info(&window);
    let settings = state.update(|settings| settings.migrate(locale.to_lowercase().starts_with("pt"), &display));
    let arguments: Vec<String> = std::env::args().collect();
    Bootstrap {
        settings,
        version: env!("CARGO_PKG_VERSION"),
        platform: if cfg!(windows) { "windows" } else { "linux" },
        display,
        graphics_cards: gpu::graphics_cards(),
        data_directory: state.paths.data_dir().to_string_lossy().into_owned(),
        arguments: LaunchArguments {
            // A direct start already tried: the window opened because it could not happen, not to press PLAY again.
            play: !state.direct_attempted.load(Ordering::SeqCst)
                && arguments.iter().skip(1).any(|a| a.eq_ignore_ascii_case(direct::PLAY_ARGUMENT)),
            wizard: arguments.iter().skip(1).any(|a| a.eq_ignore_ascii_case(direct::WIZARD_ARGUMENT)),
        },
        launcher_path: std::env::current_exe().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default(),
        notice: state.notice.lock().unwrap().take(),
    }
}

/// Merges a partial settings object, saves it and returns the whole.
#[tauri::command]
fn patch_settings(app: AppHandle, state: State<'_, AppState>, patch: serde_json::Value) -> Result<Settings, String> {
    let serde_json::Value::Object(patch) = patch else {
        return Err("the patch must be an object".into());
    };
    let reconfigure = patch.keys().any(|key| key.starts_with("optimiz"));
    let patch_turns_on_auto_update = patch.get("autoUpdate") == Some(&serde_json::Value::Bool(true));
    let mut error = None;
    let settings = state.update(|settings| {
        let mut value = serde_json::to_value(&*settings).unwrap_or_default();
        if let serde_json::Value::Object(map) = &mut value {
            map.extend(patch);
        }
        match serde_json::from_value::<Settings>(value) {
            Ok(merged) => *settings = merged,
            Err(failure) => error = Some(failure.to_string()),
        }
    });
    if let Some(error) = error {
        return Err(error);
    }
    if reconfigure {
        configure_optimizer(&state, &settings);
    }
    if settings.auto_update && patch_turns_on_auto_update {
        updater::nudge(&app);
    }
    Ok(settings)
}

fn configure_optimizer(state: &AppState, settings: &Settings) {
    state.optimizer.configure(
        &settings.optimization_destination,
        settings.optimization_enabled,
        settings.optimize_during_game,
        settings.optimization_compiled,
    );
}

#[tauri::command]
fn reset_game_options(window: WebviewWindow, state: State<'_, AppState>) -> Settings {
    let display = display_info(&window);
    state.update(|settings| settings.reset_game_options(&display))
}

// ---------------------------------------------------------------------------------------------- game files

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameStatus {
    game: GameState,
    data: DataState,
    executable: ExecutableState,
    ready: bool,
}

#[tauri::command(async)]
fn game_status(state: State<'_, AppState>) -> GameStatus {
    let settings = state.snapshot();
    let game = game::check_game(&settings.elf, &settings.executable);
    let data = game::check_data(&settings.elf, &settings.iso);
    let executable = game::check_executable(&settings.executable);
    let ready = game.valid && data.ok && executable.executable;
    GameStatus { game, data, executable, ready }
}

/// Looks for the ELF and the runner. Without `force`, only the empty fields are filled.
#[tauri::command(async)]
fn detect_paths(state: State<'_, AppState>, force: bool) -> Settings {
    let found: DetectedPaths = game::detected_paths(&launcher_dir(), &gow_core::paths::default_destination());
    state.update(|settings| {
        if (force || settings.elf.is_empty()) && !found.elf.is_empty() {
            settings.elf = found.elf.clone();
        }
        if (force || settings.executable.is_empty()) && !found.executable.is_empty() {
            settings.executable = found.executable.clone();
        }
    })
}

#[tauri::command(async)]
fn detected_iso() -> String {
    game::detected_paths(&launcher_dir(), &gow_core::paths::default_destination()).iso
}

#[tauri::command]
fn memory_card_file(elf: String) -> String {
    game::memory_card_file(&elf)
}

// ---------------------------------------------------------------------------------------------- saves

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The saves the memory card holds, in the order the game lists them.
#[tauri::command(async)]
fn card_slots(elf: String) -> Vec<gow_core::saves::CardSlot> {
    let card = game::memory_card_file(&elf);
    if card.is_empty() {
        Vec::new()
    } else {
        gow_core::saves::read_card_file_slots(std::path::Path::new(&card))
    }
}

/// The dated copies of the card the game made at its altars (newest first).
#[tauri::command(async)]
fn save_backups(elf: String) -> Vec<gow_core::saves::SaveBackup> {
    let card = game::memory_card_file(&elf);
    if card.is_empty() {
        Vec::new()
    } else {
        gow_core::saves::list_backups(std::path::Path::new(&card))
    }
}

/// The saves inside one of those copies.
#[tauri::command(async)]
fn save_backup_slots(elf: String, id: String) -> Vec<gow_core::saves::CardSlot> {
    let card = game::memory_card_file(&elf);
    if card.is_empty() {
        Vec::new()
    } else {
        gow_core::saves::backup_slots(std::path::Path::new(&card), &id)
    }
}

/// A copy of the card now (a manual one).
#[tauri::command(async)]
fn backup_card_now(elf: String) -> Result<gow_core::saves::SaveBackup, String> {
    let card = game::memory_card_file(&elf);
    gow_core::saves::make_backup(std::path::Path::new(&card), "manual", unix_now()).map_err(|error| error.to_string())
}

/// Puts a copy in place of the card. Refused while the game runs; the card it replaces is kept in the history first.
#[tauri::command(async)]
fn restore_save_backup(elf: String, id: String, state: State<'_, AppState>) -> Result<(), String> {
    let card = game::memory_card_file(&elf);
    let running = state.game.lock().unwrap().is_some();
    gow_core::saves::restore_backup(std::path::Path::new(&card), &id, running, unix_now())
        .map_err(|error| error.to_string())
}

/// The names of the save points and difficulties and the thumbnails of `points`, read from the player's installed disc
/// (empty without it). Off the main thread: it reads about 4 MB.
#[tauri::command]
async fn save_art(state: State<'_, AppState>, points: Vec<u16>) -> Result<gow_core::save_art::SaveArt, String> {
    let settings = state.snapshot();
    let (elf, iso) = (settings.elf.clone(), settings.iso.clone());
    tauri::async_runtime::spawn_blocking(move || gow_core::save_art::read_save_art(&elf, &iso, &points))
        .await
        .map_err(|error| error.to_string())
}

/// Asks the game for one slot of the card. While it runs it goes there from wherever it is; otherwise the request waits
/// for the next start (the page then starts the game). Returns whether the game is running.
#[tauri::command(async)]
fn load_save_slot(elf: String, index: usize, state: State<'_, AppState>) -> Result<bool, String> {
    let card = game::memory_card_file(&elf);
    gow_core::saves::request_load(std::path::Path::new(&card), index, unix_now()).map_err(|error| error.to_string())?;
    Ok(state.game.lock().unwrap().is_some())
}

// ---------------------------------------------------------------------------------------------- playing

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    key: String,
    detail: String,
}

impl From<LaunchError> for CommandError {
    fn from(error: LaunchError) -> Self {
        Self { key: error.key().to_string(), detail: error.detail().to_string() }
    }
}

#[tauri::command]
fn start_game(app: AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    launch_game(&app, &state)
}

/// Opens the game: the PLAY button and a direct start (issue #40) go through here, so both build the same environment,
/// track the session and count the play time.
fn launch_game(app: &AppHandle, state: &AppState) -> Result<(), CommandError> {
    if state.game.lock().unwrap().is_some() {
        return Ok(());
    }
    // GOW-Port: "Optimize now" opens its own game; a second one would cancel it (and the training build would be left behind).
    if state.optimizer.snapshot().profiling {
        return Err(CommandError { key: "errorProfiling".into(), detail: String::new() });
    }
    // If a rebuild finished since last time, the active copy of the executable is updated before opening.
    state.optimizer.prepare_to_play();
    // What the player changed inside the game last time and the launcher did not get to read (it was closed meanwhile).
    let ingame_state = state.paths.ingame_state_file();
    state.update_if(|s| gow_core::ingame::consume(s, &ingame_state));
    let settings = state.snapshot();
    // GOW-Port: the instrumented training build of an interrupted "Optimize now" is several times slower than the game: it is
    // never opened (the interface explains it and the optimizer rebuilds the game by itself).
    if gow_core::optimizer::is_instrumented(Path::new(&settings.executable)) {
        return Err(CommandError { key: "errorInstrumented".into(), detail: settings.executable.clone() });
    }
    let options = StartOptions { vu1_capture: state.optimizer.capture_directory() };
    let finished_app = app.clone();
    // Written before the game starts, so a session that ends at once cannot be overwritten by a late "started".
    let started_at = report::unix_now();
    let previous_session = session::read_session(&state.paths);
    session::session_started(&state.paths, started_at);
    let started = process::start(&settings, &options, &state.paths, move |finished| {
        let state = finished_app.state::<AppState>();
        *state.game.lock().unwrap() = None;
        session::session_ended(&state.paths, started_at, &finished, report::unix_now());
        let ingame_state = state.paths.ingame_state_file();
        let updated = state.update(|s| {
            s.seconds_played += finished.seconds;
            s.last_session = finished.seconds;
            // the cheats, overlay lines and image adjustments the player set inside the game start the next session
            gow_core::ingame::consume(s, &ingame_state);
        });
        state.optimizer.game_state_changed(false);
        state.started_directly.store(false, Ordering::SeqCst);
        // GOW-Port (issue #40): started directly and no window was ever opened: the launcher ends with the game (Steam sees the
        // session end), unless the game failed, which opens the window with the reason.
        if state.windowless.load(Ordering::SeqCst) {
            if finished.failed {
                *state.notice.lock().unwrap() = Some(StartNotice { finished: Some(finished), launch_error: None });
                open_window(&finished_app);
            } else {
                state.optimizer.shutdown();
                finished_app.exit(0);
            }
            return;
        }
        if let Some(window) = finished_app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        let _ = finished_app.emit("settings-changed", updated);
        let _ = finished_app.emit("game-running", false);
        let _ = finished_app.emit("game-finished", finished);
    });
    let session = match started {
        Ok(session) => session,
        Err(error) => {
            session::session_not_started(&state.paths, previous_session);
            return Err(error.into());
        }
    };
    *state.game.lock().unwrap() = Some(session);
    state.optimizer.game_state_changed(true);
    let _ = app.emit("game-running", true);
    if settings.minimize_on_play {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            if app.state::<AppState>().game.lock().unwrap().is_some() {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.minimize();
                }
            }
        });
    }
    Ok(())
}

#[tauri::command]
fn stop_game(state: State<'_, AppState>) {
    if let Some(session) = *state.game.lock().unwrap() {
        session.stop();
    }
}

#[tauri::command]
fn is_game_running(state: State<'_, AppState>) -> bool {
    state.game.lock().unwrap().is_some()
}

/// Closes the window for good, skipping the check that holds it open while a texture pack is being installed. The
/// interface calls it after the player confirmed (see `close-blocked`).
#[tauri::command]
fn force_close(window: WebviewWindow) {
    let _ = window.destroy();
}

// ---------------------------------------------------------------------------------------------- shell helpers

#[tauri::command(async)]
fn open_log(state: State<'_, AppState>) {
    let log = state.paths.last_log();
    let target = if log.exists() { log } else { state.paths.logs_dir() };
    let _ = shortcut::open_path(&target);
}

#[tauri::command(async)]
fn open_install_log(state: State<'_, AppState>) {
    let _ = shortcut::open_path(&state.paths.logs_dir().join("install.log"));
}

#[tauri::command(async)]
fn open_optimizer_log(state: State<'_, AppState>) {
    let _ = shortcut::open_path(&state.paths.logs_dir().join("optimize.log"));
}

// ---------------------------------------------------------------------------------------------- session report

/// Writes the report ZIP (see `gow_core::report`), opens its folder and returns the file name. Hashing the runner and
/// scrubbing the logs take a moment, so it leaves the UI thread.
#[tauri::command]
async fn report_export(window: WebviewWindow, state: State<'_, AppState>) -> Result<String, String> {
    let settings = state.snapshot();
    let display = display_info(&window);
    let paths = state.paths.clone();
    let optimizer = state.optimizer.snapshot();
    let vu1_capture = state.optimizer.capture_directory();
    let game_running = state.game.lock().unwrap().is_some();
    let exported = tauri::async_runtime::spawn_blocking(move || {
        let host = HostFacts::detect();
        let env: Vec<(String, String)> = std::env::vars_os()
            .map(|(name, value)| (name.to_string_lossy().into_owned(), value.to_string_lossy().into_owned()))
            .collect();
        let cards = gpu::graphics_cards();
        let request = ReportRequest {
            paths: &paths,
            settings: &settings,
            launcher_version: env!("CARGO_PKG_VERSION"),
            display,
            cards: &cards,
            optimizer: &optimizer,
            game_running,
            vu1_capture: &vu1_capture,
            now: report::unix_now(),
        };
        report::export(&request, &host, &env)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    let _ = shortcut::open_path(&report::reports_dir(&state.paths));
    Ok(exported.name)
}

/// Stores a note about a problem for the next report. False when the text was empty.
#[tauri::command]
fn report_mark(state: State<'_, AppState>, text: String) -> Result<bool, String> {
    let running = state.game.lock().unwrap().is_some();
    session::add_marker(&state.paths, &text, report::unix_now(), running)
        .map(|marker| marker.is_some())
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn open_directory(path: String) {
    let target = shortcut::folder_of(Path::new(&path));
    let _ = std::fs::create_dir_all(&target);
    let _ = shortcut::open_path(&target);
}

/// Opens the `textures` folder of the installation (creating it with its `replacements` folder, as the installer does).
/// False when no game files are set yet.
#[tauri::command(async)]
fn open_textures_folder(state: State<'_, AppState>) -> bool {
    let elf = state.snapshot().elf;
    let (Some(folder), Some(replacements)) = (game::textures_folder(&elf), game::texture_replacements_folder(&elf))
    else {
        return false;
    };
    let _ = std::fs::create_dir_all(&replacements);
    shortcut::open_path(&folder).is_ok()
}

/// Size/date of the community pack and what is installed now. The metadata request leaves the UI thread.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TexturePackInfo {
    meta: Option<textures::PackMeta>,
    status: textures::PackStatus,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TexturePackOutcome {
    ok: bool,
    status: Option<textures::PackStatus>,
    error: Option<String>,
    /// Stable code the interface turns into a translated message.
    code: Option<String>,
}

#[tauri::command]
async fn texture_pack_info(state: State<'_, AppState>) -> Result<TexturePackInfo, String> {
    let elf = state.snapshot().elf;
    let source = textures::PackSource::default();
    let (meta, status) =
        tauri::async_runtime::spawn_blocking(move || (textures::meta(&source).ok(), textures::status(&elf)))
            .await
            .map_err(|error| error.to_string())?;
    Ok(TexturePackInfo { meta, status })
}

/// Downloads the pack, extracts it over `textures/replacements` and deletes the archive, reporting progress as events.
#[tauri::command]
fn texture_pack_install(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let elf = state.snapshot().elf;
    if state.textures_running.swap(true, Ordering::SeqCst) {
        return Err("a texture download is already running".into());
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let source = textures::PackSource::default();
        let progress_app = handle.clone();
        let result = textures::install(&elf, &source, &|progress| {
            let _ = progress_app.emit("texture-pack-progress", progress);
        });
        handle.state::<AppState>().textures_running.store(false, Ordering::SeqCst);
        let outcome = match result {
            Ok(status) => TexturePackOutcome { ok: true, status: Some(status), error: None, code: None },
            Err(error) => TexturePackOutcome {
                ok: false,
                status: None,
                error: Some(error.to_string()),
                code: Some(error.code().into()),
            },
        };
        let _ = handle.emit("texture-pack-done", outcome);
    });
    Ok(())
}

#[tauri::command(async)]
fn create_shortcut(state: State<'_, AppState>, desktop: bool) -> bool {
    let Ok(executable) = std::env::current_exe() else { return false };
    shortcut::create(&executable, &state.paths.icon(), desktop).is_ok()
}

// ---------------------------------------------------------------------------------------------- RetroAchievements

#[tauri::command(async)]
fn ra_has_credential(user: String) -> bool {
    credential::has_retro_achievements(&user)
}

#[tauri::command(async)]
fn ra_save_credential(user: String, password: String) -> bool {
    credential::save_retro_achievements(&user, &password)
}

#[tauri::command(async)]
fn ra_clear_credential() -> bool {
    credential::clear_retro_achievements()
}

// ---------------------------------------------------------------------------------------------- gamepad

/// The joysticks the game saw the last time it ran (the controller picker's list); `None` before the first run.
#[tauri::command(async)]
fn pad_devices(state: State<'_, AppState>) -> Option<PadDeviceList> {
    controller::read(&state.paths.pad_devices_file())
}

/// The raw input of the joysticks right now, from the game (only while the mapping dialog keeps the probe fresh); `None` when the
/// game has not written it.
#[tauri::command]
fn pad_live(state: State<'_, AppState>) -> Option<controller::LiveState> {
    controller::read_live(&state.paths.pad_live_file())
}

/// The mapping dialog asks the game for the live feed (`true`, repeated every few seconds while it is open) or stops asking.
#[tauri::command]
fn pad_probe(state: State<'_, AppState>, active: bool) {
    let file = state.paths.pad_probe_file();
    if active {
        let _ = controller::touch_probe(&file);
    } else {
        controller::clear_probe(&file);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GamepadFailure {
    reason: &'static str,
}

#[tauri::command]
fn gamepad_begin(app: AppHandle, state: State<'_, AppState>) -> bool {
    if !gamepad::supported() {
        let _ = app.emit("gamepad-failed", GamepadFailure { reason: "unsupported" });
        return false;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(previous) = state.gamepad_cancel.lock().unwrap().replace(cancel.clone()) {
        previous.store(true, Ordering::Relaxed);
    }
    std::thread::spawn(move || {
        let mut capture = Capture::new();
        while !cancel.load(Ordering::Relaxed) {
            if let Some(result) = capture.poll(gamepad::read_buttons(), Instant::now()) {
                match result {
                    CaptureResult::Captured(captured) => {
                        let _ = app.emit("gamepad-captured", captured);
                    }
                    CaptureResult::Unknown => {
                        let _ = app.emit("gamepad-failed", GamepadFailure { reason: "unknown" });
                    }
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    });
    true
}

#[tauri::command]
fn gamepad_cancel(state: State<'_, AppState>) {
    if let Some(cancel) = state.gamepad_cancel.lock().unwrap().take() {
        cancel.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------------------------- installation wizard

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WizardInfo {
    environment: installer::Environment,
    has_manifest: bool,
    default_destination: String,
}

#[tauri::command(async)]
fn wizard_info(state: State<'_, AppState>) -> WizardInfo {
    let script = install_script(&state);
    let mut environment = installer::check_environment(script.as_deref());
    environment.kit_missing = kit::missing_for_script(script.as_deref(), &state.paths.kit_dir());
    WizardInfo {
        environment,
        has_manifest: script.as_deref().is_some_and(installer::has_manifest),
        default_destination: installer::default_destination(),
    }
}

#[tauri::command(async)]
fn validate_iso(path: String) -> IsoValidation {
    iso::validate_iso(&path)
}

#[tauri::command(async)]
fn validate_destination(path: String) -> installer::DestinationValidation {
    installer::validate_destination(&path)
}

// ------------------------------------------------------------------------------------- translation patch in the wizard

/// GOW-Port (0.3.0): checks a chosen `.xdelta` from its header; `None` when it can be applied.
#[tauri::command(async)]
fn iso_patch_check(path: String) -> Option<PatchError> {
    iso_patch::check_patch(Path::new(path.trim())).err()
}

/// GOW-Port (0.3.0): applies the patch to the original ISO in the background. Reports `iso-patch-progress` (a fraction) and
/// `iso-patch-done` (a [`PatchOutcome`]); the translated ISO is written next to the original, which is only read.
#[tauri::command]
fn iso_patch_start(app: AppHandle, state: State<'_, AppState>, iso: String, patch: String) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut running = state.patch_cancel.lock().unwrap();
        if running.is_some() {
            return Err("busy".into());
        }
        *running = Some(cancel.clone());
    }
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let mut last = -1.0f64;
        let outcome: PatchOutcome = iso_patch::apply(Path::new(iso.trim()), Path::new(patch.trim()), |fraction| {
            if fraction - last >= 0.005 || fraction >= 1.0 {
                last = fraction;
                let _ = progress_app.emit("iso-patch-progress", fraction);
            }
            !cancel.load(Ordering::SeqCst)
        });
        *app.state::<AppState>().patch_cancel.lock().unwrap() = None;
        let _ = app.emit("iso-patch-done", outcome);
    });
    Ok(())
}

/// GOW-Port (0.3.0): stops the patch being applied; `iso-patch-done` then reports `cancelled` and nothing is left behind.
#[tauri::command]
fn iso_patch_cancel(state: State<'_, AppState>) {
    if let Some(cancel) = state.patch_cancel.lock().unwrap().as_ref() {
        cancel.store(true, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------------------------------- CHD disc image in the wizard

/// GOW-Port (0.3.1): where the ISO of a chosen `.chd` goes and whether it fits; `None` when the CHD cannot be read.
#[tauri::command(async)]
fn iso_chd_plan(path: String) -> Option<ChdPlan> {
    iso_chd::plan(Path::new(path.trim())).ok()
}

/// GOW-Port (0.3.1): converts the CHD to an ISO next to it in the background. Reports `iso-chd-progress` (a fraction)
/// and `iso-chd-done` (a [`ChdOutcome`]); the CHD is only read.
#[tauri::command]
fn iso_chd_start(app: AppHandle, state: State<'_, AppState>, chd: String) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut running = state.chd_cancel.lock().unwrap();
        if running.is_some() {
            return Err("busy".into());
        }
        *running = Some(cancel.clone());
    }
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let mut last = -1.0f64;
        let outcome: ChdOutcome = iso_chd::convert(Path::new(chd.trim()), |fraction| {
            if fraction - last >= 0.005 || fraction >= 1.0 {
                last = fraction;
                let _ = progress_app.emit("iso-chd-progress", fraction);
            }
            !cancel.load(Ordering::SeqCst)
        });
        *app.state::<AppState>().chd_cancel.lock().unwrap() = None;
        let _ = app.emit("iso-chd-done", outcome);
    });
    Ok(())
}

/// GOW-Port (0.3.1): stops the conversion; `iso-chd-done` then reports `cancelled` and nothing is left behind.
#[tauri::command]
fn iso_chd_cancel(state: State<'_, AppState>) {
    if let Some(cancel) = state.chd_cancel.lock().unwrap().as_ref() {
        cancel.store(true, Ordering::SeqCst);
    }
}

/// Searches the PC for the original ISO in the background; reports through `iso-searching` and `iso-found`.
#[tauri::command]
fn find_iso(app: AppHandle, state: State<'_, AppState>) {
    if state.searching.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = app.emit("iso-searching", true);
    std::thread::spawn(move || {
        let found = iso::find_original_iso();
        app.state::<AppState>().searching.store(false, Ordering::Relaxed);
        let _ = app.emit("iso-searching", false);
        if let Some(found) = found {
            let _ = app.emit("iso-found", found.to_string_lossy().into_owned());
        }
    });
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallArguments {
    iso: String,
    destination: String,
    install_tools: bool,
    vu1_memory_limit_gb: u8,
}

#[tauri::command]
fn install_start(app: AppHandle, state: State<'_, AppState>, arguments: InstallArguments) -> Result<(), String> {
    if state.installation.lock().unwrap().is_some() {
        return Err("an installation is already running".into());
    }
    let script = install_script(&state).ok_or("install.ps1 was not found next to the launcher.")?;
    let script_used = script.clone();
    let destination_used = arguments.destination.clone();
    let request = InstallRequest {
        script,
        iso: arguments.iso,
        destination: arguments.destination,
        install_tools: arguments.install_tools,
        vu1_memory_limit_gb: arguments.vu1_memory_limit_gb,
        log_file: state.paths.logs_dir().join("install.log"),
    };
    let (progress_app, done_app) = (app.clone(), app);
    let installation = installer::start(
        request,
        move |progress| {
            let _ = progress_app.emit("install-progress", progress);
        },
        move |mut outcome| {
            let state = done_app.state::<AppState>();
            *state.installation.lock().unwrap() = None;
            // A kit file that is gone explains any failure better than the script's own message.
            if !outcome.ok
                && !outcome.cancelled
                && !kit::missing_for_script(Some(&script_used), &state.paths.kit_dir()).is_empty()
            {
                outcome.failure_kind = Some(FailureKind::KitFile.key().to_string());
            }
            // The build was made from the kit's sources: record it, so no rebuild is offered for what was just installed.
            if outcome.ok && script_used.starts_with(state.paths.kit_dir()) {
                rebuild::record_build(&state.paths.kit_dir(), Path::new(&destination_used));
            }
            let _ = done_app.emit("install-done", outcome);
        },
    )
    .map_err(|error| format!("Could not start PowerShell: {error}"))?;
    *state.installation.lock().unwrap() = Some(installation);
    Ok(())
}

#[tauri::command]
fn install_cancel(state: State<'_, AppState>) {
    if let Some(installation) = state.installation.lock().unwrap().as_ref() {
        installation.cancel();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteArguments {
    destination: String,
    elf: String,
    executable: String,
}

/// The wizard succeeded: the game now runs from a copy (`<destination>/active`), so the optimizer can rebuild without
/// locking the executable. The ISO is no longer needed.
#[tauri::command]
fn install_complete(state: State<'_, AppState>, arguments: CompleteArguments) -> Settings {
    let active = optimizer::register_installation(Path::new(&arguments.executable), Path::new(&arguments.destination));
    state.optimizer.registered(&arguments.destination);
    let compiled = state.optimizer.snapshot().compiled;
    let settings = state.update(|settings| {
        settings.optimization_destination = arguments.destination.clone();
        settings.optimization_compiled = compiled;
        settings.elf = arguments.elf.clone();
        settings.executable = active.to_string_lossy().into_owned();
        settings.iso.clear();
    });
    configure_optimizer(&state, &settings);
    settings
}

// ---------------------------------------------------------------------------------------------- optimizer

#[tauri::command]
fn optimizer_snapshot(state: State<'_, AppState>) -> OptimizerSnapshot {
    state.optimizer.snapshot()
}

#[tauri::command]
fn optimize_now(state: State<'_, AppState>) {
    state.optimizer.optimize_now();
}

// ---------------------------------------------------------------------------------------------- rebuild after an update

/// Whether the game build in the installation folder is older than the runtime sources of the kit (see `gow_core::rebuild`).
#[tauri::command(async)]
fn rebuild_state(state: State<'_, AppState>) -> rebuild::RebuildState {
    let destination = state.snapshot().optimization_destination;
    if destination.trim().is_empty() {
        return rebuild::RebuildState::default();
    }
    let current = rebuild::state(&state.paths.kit_dir(), Path::new(destination.trim()));
    rebuild::announce(state.paths.data_dir(), current)
}

/// Starts the rebuild the player asked for. Fails at once, with a code, when it must not start (`busy:game`,
/// `busy:installation`, `busy:optimizer`, `busy:texturePack`: the same reasons as a launcher update, since a rebuild takes the
/// CPU and replaces the files the game runs from; `notAvailable`).
#[tauri::command]
fn rebuild_start(state: State<'_, AppState>) -> Result<(), String> {
    let activity = Activity {
        game: state.game.lock().unwrap().is_some(),
        installation: state.installation.lock().unwrap().is_some(),
        optimizer: state.optimizer.snapshot().compiling,
        texture_pack: state.textures_running.load(Ordering::SeqCst),
    };
    if let Some(blocker) = activity.blocker() {
        return Err(format!("busy:{}", blocker.code()));
    }
    if state.optimizer.rebuild_now() {
        Ok(())
    } else {
        Err("notAvailable".into())
    }
}

/// GOW-Port: "Optimize now": the speed profile (PGO) and the native IOP modules, with the game opening by itself for a few
/// minutes, then a build that uses them. Refused like a rebuild (`busy:game`, `busy:installation`, `busy:optimizer`,
/// `busy:texturePack`), and `notAvailable` when the installation cannot have a profile (not Windows, or not built with clang).
#[tauri::command]
fn profile_start(state: State<'_, AppState>) -> Result<(), String> {
    let activity = Activity {
        game: state.game.lock().unwrap().is_some(),
        installation: state.installation.lock().unwrap().is_some(),
        optimizer: state.optimizer.snapshot().compiling,
        texture_pack: state.textures_running.load(Ordering::SeqCst),
    };
    if let Some(blocker) = activity.blocker() {
        return Err(format!("busy:{}", blocker.code()));
    }
    if state.optimizer.profile_now() {
        Ok(())
    } else {
        Err("notAvailable".into())
    }
}

/// GOW-Port: the player cancelled "Optimize now" (after confirming). Its game closes with it; a training build already made is
/// rebuilt into a normal one by the optimizer.
#[tauri::command]
fn profile_cancel(state: State<'_, AppState>) -> bool {
    state.optimizer.cancel_profile()
}

// ---------------------------------------------------------------------------------------------- self-update

#[tauri::command]
fn update_state(state: State<'_, AppState>) -> gow_core::update::UpdateState {
    state.updater.snapshot()
}

/// Downloads and installs the update found by the check, then restarts. Fails at once, with a code, when it must not
/// start (`busy:game`, `busy:installation`, `busy:optimizer`, `busy:texturePack`, `notAvailable`).
#[tauri::command]
fn update_install(app: AppHandle) -> Result<(), String> {
    updater::install(&app)
}

#[tauri::command]
fn update_open_release(app: AppHandle) -> bool {
    updater::open_release(&app)
}

// ---------------------------------------------------------------------------------------------- startup

/// When `run` began, and how long the setup hook took (both for `logs/launcher-startup.log`).
static STARTED: OnceLock<Instant> = OnceLock::new();
static SETUP_MS: AtomicU64 = AtomicU64::new(0);

/// The interface drew its first screen with data. One line per start in `logs/launcher-startup.log` (the newest 200 are
/// kept), so a report of a slow launcher says where the time went: the setup before the window, the page's own start
/// (`script_ms`: WebView2 and the bundle, from the page's navigation) and the first calls (`boot_ms`).
#[tauri::command(async)]
fn startup_ready(state: State<'_, AppState>, script_ms: f64, boot_ms: f64) {
    let total = STARTED.get().map_or(0, |started| started.elapsed().as_millis());
    let line = format!(
        "[startup] {} interface ready {total} ms after start (setup {} ms, page script at {script_ms:.0} ms, first calls {boot_ms:.0} ms)",
        report::unix_now(),
        SETUP_MS.load(Ordering::Relaxed),
    );
    append_startup_log(&state.paths, line);
}

fn append_startup_log(paths: &AppPaths, line: String) {
    let file = paths.logs_dir().join("launcher-startup.log");
    let mut lines: Vec<String> =
        std::fs::read_to_string(&file).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default();
    lines.push(line);
    let keep = lines.len().saturating_sub(200);
    let _ = std::fs::write(&file, lines[keep..].join("\n") + "\n");
}

fn prepare_data_directory(paths: &AppPaths) {
    let _ = std::fs::create_dir_all(paths.logs_dir());
    if !paths.icon().exists() {
        let _ = std::fs::write(paths.icon(), ICON);
    }
}

/// The installation kit travels inside the launcher's installer (a Tauri resource); it is extracted into the data
/// folder, which `install.ps1` can write to. Not bundled (development): the kit is found in the repository instead.
fn extract_kit(app: &tauri::App, paths: &AppPaths) {
    let Ok(resources) = app.path().resource_dir() else { return };
    let bundled = resources.join("kit");
    if bundled.is_dir() {
        let (kit_dir, had_marker) = (paths.kit_dir(), paths.kit_dir().join(kit::MARKER).is_file());
        let missing_before = kit::missing_essentials(&kit_dir);
        let _ = kit::sync(&bundled, &kit_dir, env!("CARGO_PKG_VERSION"));
        // A first extraction or an update also finds files "missing"; only a kit that had been extracted before and lost
        // files is a repair worth a line.
        let restored = missing_before.len().saturating_sub(kit::missing_essentials(&kit_dir).len());
        if had_marker && restored > 0 {
            log_kit(paths, &format!("[kit] restored {restored} missing files: {}", missing_before.join(", ")));
        }
    }
}

/// Appends a line to `logs/kit.log`, so a player's report shows that the kit repaired itself.
fn log_kit(paths: &AppPaths, line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(paths.logs_dir().join("kit.log")) {
        let _ = writeln!(file, "{line}");
    }
}

/// Shows the launcher's window, creating it the first time (it is not created at start-up: a direct start never needs it, and
/// skipping it skips WebView2).
fn open_window(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.windowless.store(false, Ordering::SeqCst);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let Some(config) = app.config().app.windows.iter().find(|window| window.label == "main") else { return };
    if let Err(error) = tauri::WebviewWindowBuilder::from_config(app, config).and_then(|builder| builder.build()) {
        if let Some(state) = app.try_state::<AppState>() {
            append_startup_log(
                &state.paths,
                format!("[startup] {} the window could not be created: {error}", report::unix_now()),
            );
        }
    }
}

/// GOW-Port (issue #40): opens the game without the window. `false` when the window has to open instead: the game is not
/// ready (the window opens the wizard) or could not start (the window shows why).
fn start_directly(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.direct_attempted.store(true, Ordering::SeqCst);
    if let Some(reason) = direct::not_ready(&state.snapshot()) {
        append_startup_log(
            &state.paths,
            format!("[startup] {} direct start: not ready ({reason}), opening the launcher", report::unix_now()),
        );
        return false;
    }
    state.windowless.store(true, Ordering::SeqCst);
    match launch_game(app, &state) {
        Ok(()) => {
            state.started_directly.store(true, Ordering::SeqCst);
            append_startup_log(
                &state.paths,
                format!(
                    "[startup] {} direct start: game opened {} ms after start",
                    report::unix_now(),
                    STARTED.get().map_or(0, |started| started.elapsed().as_millis())
                ),
            );
            true
        }
        Err(error) => {
            state.windowless.store(false, Ordering::SeqCst);
            *state.notice.lock().unwrap() = Some(StartNotice { finished: None, launch_error: Some(error) });
            false
        }
    }
}

/// A second start of the launcher while one is running (the single-instance plugin hands its arguments here): a direct
/// start opens the game if it is not open yet, anything else shows the window.
fn second_start(app: &AppHandle, arguments: &[String]) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let mode = direct::start_mode(arguments, state.snapshot().skip_launcher, direct::launcher_key_held());
    if mode == direct::StartMode::Direct {
        if state.game.lock().unwrap().is_some() {
            return;
        }
        if direct::not_ready(&state.snapshot()).is_none() {
            match launch_game(app, &state) {
                Ok(()) => return,
                Err(error) => {
                    let notice = StartNotice { finished: None, launch_error: Some(error) };
                    // A window that is already open hears it now; a new one reads it when it boots.
                    if app.get_webview_window("main").is_some() {
                        let _ = app.emit("start-notice", notice);
                    } else {
                        *state.notice.lock().unwrap() = Some(notice);
                    }
                }
            }
        }
    }
    open_window(app);
}

fn load_settings(paths: &AppPaths) -> Settings {
    if paths.settings_file().exists() {
        return Settings::load(&paths.settings_file());
    }
    // First run of this launcher: bring over what the Qt launcher stored.
    let imported = legacy::import_previous().unwrap_or_default();
    let _ = imported.save(&paths.settings_file());
    imported
}

pub fn run() {
    STARTED.get_or_init(Instant::now);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _directory| {
            // Off the event loop: it may start the game or build the window.
            let app = app.clone();
            std::thread::spawn(move || second_start(&app, &arguments));
        }))
        .setup(|app| {
            let setup_started = Instant::now();
            let paths = AppPaths::system();
            prepare_data_directory(&paths);
            extract_kit(app, &paths);
            let settings = load_settings(&paths);

            let (notify_app, compiled_app) = (app.handle().clone(), app.handle().clone());
            let optimizer = Optimizer::new(
                paths.logs_dir().join("optimize.log"),
                move |snapshot| {
                    let _ = notify_app.emit("optimizer", snapshot);
                },
                move |compiled| {
                    let state = compiled_app.state::<AppState>();
                    let updated = state.update(|s| s.optimization_compiled = compiled);
                    let _ = compiled_app.emit("settings-changed", updated);
                },
            );
            let paths_logs = paths.logs_dir();
            let state = AppState {
                paths,
                settings: Mutex::new(settings.clone()),
                game: Mutex::new(None),
                installation: Mutex::new(None),
                optimizer,
                gamepad_cancel: Mutex::new(None),
                searching: AtomicBool::new(false),
                textures_running: AtomicBool::new(false),
                patch_cancel: Mutex::new(None),
                chd_cancel: Mutex::new(None),
                updater: updater::Updates::new(paths_logs.join("update.log")),
                windowless: AtomicBool::new(false),
                started_directly: AtomicBool::new(false),
                direct_attempted: AtomicBool::new(false),
                notice: Mutex::new(None),
            };
            state.optimizer.set_script(install_script(&state));
            configure_optimizer(&state, &settings);
            state.optimizer.spawn_timer();
            app.manage(state);
            updater::spawn_scheduler(app.handle().clone());
            SETUP_MS.store(setup_started.elapsed().as_millis() as u64, Ordering::Relaxed);
            // GOW-Port (issue #40): the window is created here (`"create": false` in tauri.conf.json), unless the game opens
            // directly.
            let arguments: Vec<String> = std::env::args().collect();
            let mode = direct::start_mode(&arguments, settings.skip_launcher, direct::launcher_key_held());
            if mode != direct::StartMode::Direct || !start_directly(app.handle()) {
                open_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // A texture pack takes minutes to install and a close (the X, Alt+F4, the taskbar) would throw it away: the
            // window stays open and the interface asks the player first.
            // GOW-Port: the same for "Optimize now": closing the launcher kills it, and the game it opened, halfway.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<AppState>();
                if state.textures_running.load(Ordering::SeqCst) || state.optimizer.snapshot().profiling {
                    api.prevent_close();
                    let _ = window.emit("close-blocked", ());
                }
            }
            if let tauri::WindowEvent::Destroyed = event {
                let state = window.app_handle().state::<AppState>();
                // The game stays open if the launcher is closed; the build tools do not.
                state.optimizer.shutdown();
                let installation = state.installation.lock().unwrap();
                if let Some(installation) = installation.as_ref() {
                    installation.cancel();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            startup_ready,
            patch_settings,
            reset_game_options,
            game_status,
            detect_paths,
            detected_iso,
            memory_card_file,
            card_slots,
            save_backups,
            save_backup_slots,
            backup_card_now,
            restore_save_backup,
            save_art,
            load_save_slot,
            start_game,
            stop_game,
            is_game_running,
            force_close,
            open_log,
            open_install_log,
            open_optimizer_log,
            report_export,
            report_mark,
            open_directory,
            open_textures_folder,
            texture_pack_info,
            texture_pack_install,
            create_shortcut,
            ra_has_credential,
            ra_save_credential,
            ra_clear_credential,
            pad_devices,
            pad_live,
            pad_probe,
            gamepad_begin,
            gamepad_cancel,
            wizard_info,
            validate_iso,
            iso_patch_check,
            iso_patch_start,
            iso_patch_cancel,
            iso_chd_plan,
            iso_chd_start,
            iso_chd_cancel,
            validate_destination,
            find_iso,
            install_start,
            install_cancel,
            install_complete,
            optimizer_snapshot,
            optimize_now,
            rebuild_state,
            rebuild_start,
            profile_start,
            profile_cancel,
            update_state,
            update_install,
            update_open_release,
        ])
        .build(tauri::generate_context!())
        .expect("error while running the launcher")
        .run(|app, event| match event {
            // A window opened during a game that was started directly: closing it waits for the game, so the play time is
            // counted and Steam does not see the game end early.
            tauri::RunEvent::ExitRequested { api, code: None, .. } => {
                if let Some(state) = app.try_state::<AppState>() {
                    if state.started_directly.load(Ordering::SeqCst) && state.game.lock().unwrap().is_some() {
                        state.windowless.store(true, Ordering::SeqCst);
                        api.prevent_exit();
                    }
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.optimizer.shutdown();
                }
            }
            _ => {}
        });
}
