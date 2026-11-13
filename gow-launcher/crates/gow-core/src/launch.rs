//! The environment the game is started with. This is the contract between the launcher and the runtime
//! (`docs/LAUNCHER.md`, "Variables passed to the game"): every line below mirrors `Launcher::start` of the former
//! C++ launcher, and the golden tests at the bottom pin it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::controller;
use crate::game;
use crate::keys;
use crate::padmap;
use crate::settings::Settings;

/// Variables of Waffle565's runtime (`run_gow.sh`): VIF1/VU1/GIF on the GS thread,
/// asynchronous FINISH, sound on the default stream and the `[gow-perf]` line every 5 s. A variable that is already
/// in the user's environment is respected, so comparisons can be made without touching the launcher.
/// `GOW_NATIVE_CHEATS_MENU=1`: the Port Options entry (with Cheats) of the game's own pause menu (docs/dev/NATIVE_MENU_RESEARCH.md). The runtime
/// leaves the stock menu alone when cheats are unavailable (RetroAchievements Hardcore) or the data is not the retail game, and
/// `GOW_NATIVE_CHEATS_MENU=0` in the user's environment turns it off.
const BASE_ENVIRONMENT: [(&str, &str); 6] = [
    ("PS2X_GS_WORKER", "1"),
    ("GOW_GS_FINISH_ASYNC", "1"),
    ("GOW_AUDIO_DEVICE", "default"),
    ("GOW_PERF_DIAG", "1"),
    ("GOW_PERF_FRAME_PC", "0x001837B8"),
    ("GOW_NATIVE_CHEATS_MENU", "1"),
];

/// What `start` needs besides the settings.
#[derive(Debug, Clone)]
pub struct LaunchContext<'a> {
    pub settings: &'a Settings,
    /// `internalResolution` resolved by [`Settings::selected_scale`].
    pub scale: f64,
    /// Folder where the game records new VU1 microprograms; empty when the optimizer is off.
    pub vu1_capture: String,
    /// The chosen GPU, already replaced by "auto" if that card is no longer present.
    pub gpu: String,
    /// `icon.png` of the data folder, if it exists.
    pub icon: Option<PathBuf>,
    pub shader_cache: PathBuf,
    /// Where the game writes the list of joysticks it sees (`GOW_PAD_LIST_FILE`); `None` leaves the variable out.
    pub pad_devices_file: Option<PathBuf>,
    /// Only read when RetroAchievements are requested.
    pub retro_achievements_password: Option<String>,
    /// Refresh rate of the display ([`crate::display::refresh_rate`]): the window refreshes at this rate.
    pub refresh_hz: u32,
}

/// Changes to apply on top of the inherited environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentPlan {
    pub set: BTreeMap<String, String>,
    pub remove: BTreeSet<String>,
}

impl EnvironmentPlan {
    fn insert(&mut self, name: &str, value: impl Into<String>) {
        self.remove.remove(name);
        self.set.insert(name.to_string(), value.into());
    }

    fn remove(&mut self, name: &str) {
        self.set.remove(name);
        self.remove.insert(name.to_string());
    }

    fn flag(&mut self, name: &str, enabled: bool, disabled_value: Option<&str>) {
        match (enabled, disabled_value) {
            (true, _) => self.insert(name, "1"),
            (false, Some(value)) => self.insert(name, value),
            (false, None) => self.remove(name),
        }
    }

    fn text_or_remove(&mut self, name: &str, value: &str) {
        if value.is_empty() {
            self.remove(name);
        } else {
            self.insert(name, value);
        }
    }

    /// Value after applying the plan to `inherited`.
    pub fn resolve(&self, name: &str, inherited: impl Fn(&str) -> Option<String>) -> Option<String> {
        if let Some(value) = self.set.get(name) {
            return Some(value.clone());
        }
        if self.remove.contains(name) {
            return None;
        }
        inherited(name)
    }
}

/// Why the game was not started. `key()` is the interface text to show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchError {
    #[error("the runner is missing or not executable: {0}")]
    Executable(String),
    #[error("the ELF is missing: {0}")]
    Elf(String),
    #[error("the ELF does not match the runner: {0}")]
    ElfMismatch(String),
    #[error("could not open the log: {0}")]
    LogOpen(String),
}

impl LaunchError {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Executable(_) => "errorExecutable",
            Self::Elf(_) => "errorElf",
            Self::ElfMismatch(_) => "errorElfMismatch",
            Self::LogOpen(_) => "errorArranque",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Executable(detail) | Self::Elf(detail) | Self::ElfMismatch(detail) | Self::LogOpen(detail) => detail,
        }
    }
}

/// Refuses to start unless the runner, the ELF and their fingerprint check out. Repeated here, not only in the UI.
pub fn validate(settings: &Settings) -> Result<(), LaunchError> {
    if !game::check_executable(&settings.executable).executable {
        return Err(LaunchError::Executable(settings.executable.clone()));
    }
    if !Path::new(&settings.elf).is_file() {
        return Err(LaunchError::Elf(settings.elf.clone()));
    }
    if !game::check_game(&settings.elf, &settings.executable).valid {
        return Err(LaunchError::ElfMismatch(settings.elf.clone()));
    }
    Ok(())
}

/// The highest frame rate the project runs the game or its window at (AGENTS.md): above it nothing gets smoother on a weak
/// PC, and the uncapped rates kept the GPU at 100 % until the AMD OpenGL driver crashed. The runtime has the same cap
/// (`kMaxFps`, ps2xRuntime/src/lib/gow_frame.h).
pub const MAX_FPS: u32 = 120;

/// The FPS limits the launcher offers (60 native, 120); any stored value snaps to the nearest one, never above `MAX_FPS`.
pub fn fps_limit(stored: i32) -> u32 {
    match stored {
        i32::MIN..=90 => 60,
        _ => MAX_FPS,
    }
}

fn fixed(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}")
}

/// Cheat-menu shortcut: one or two raylib buttons, 1..=17, joined by '+'.
pub fn cheats_shortcut_is_valid(shortcut: &str) -> bool {
    let shortcut = shortcut.trim();
    if shortcut.is_empty() {
        return false;
    }
    let parts: Vec<&str> = shortcut.split('+').collect();
    parts.len() <= 2 && parts.iter().all(|part| part.parse::<i32>().is_ok_and(|button| (1..=17).contains(&button)))
}

/// Builds the changes to the child's environment. `inherited` looks a name up in the launcher's own environment.
pub fn build_environment(ctx: &LaunchContext<'_>, inherited: impl Fn(&str) -> Option<String>) -> EnvironmentPlan {
    let settings = ctx.settings;
    let mut plan = EnvironmentPlan::default();
    let keep_inherited = |plan: &mut EnvironmentPlan, name: &str, value: &str| {
        if inherited(name).is_none() {
            plan.insert(name, value);
        }
    };
    for (name, value) in BASE_ENVIRONMENT {
        keep_inherited(&mut plan, name, value);
    }

    // Game options.
    plan.flag("GOW_SKIP_FMV", settings.skip_videos, None);
    plan.flag("GOW_FAST_BOOT", settings.fast_boot, None);
    // Custom textures: the runner indexes this folder (PCSX2's `replacements` layout) and draws what it finds there.
    match game::texture_replacements_folder(&settings.elf).filter(|_| settings.custom_textures) {
        Some(folder) => plan.insert("GOW_TEXTURE_REPLACE", folder.to_string_lossy().into_owned()),
        None => plan.remove("GOW_TEXTURE_REPLACE"),
    }
    plan.flag("GOW_TEXTURE_REPLACE_SYNC", !settings.async_textures, None);
    plan.flag("PC_DMC5_CONTROLS", settings.dmc5_controls, Some("0"));
    plan.text_or_remove("PC_PAD_FACE_MAP", &settings.pad_buttons);
    // Every gamepad button (padmap.rs); the game ignores an invalid value, so an invalid one is not passed at all.
    // The controller picker: the game lists the joysticks it sees in a file of ours and reads only the chosen device (a GUID).
    match &ctx.pad_devices_file {
        Some(file) => plan.insert("GOW_PAD_LIST_FILE", file.to_string_lossy().into_owned()),
        None => plan.remove("GOW_PAD_LIST_FILE"),
    }
    plan.text_or_remove(
        "GOW_PAD_DEVICE",
        if controller::is_guid(&settings.pad_device) { &settings.pad_device } else { "" },
    );
    // A hand-made SDL mapping for that device, when the player built one because the system has none.
    let sdl_mapping = controller::mapping_line(&settings.pad_device, &settings.pad_device_name, &settings.pad_sdl_map);
    plan.text_or_remove("GOW_PAD_SDL_MAPPING", sdl_mapping.as_deref().unwrap_or(""));
    plan.text_or_remove("PC_PAD_MAP", if padmap::pad_map_is_valid(&settings.pad_map) { &settings.pad_map } else { "" });
    plan.flag("PC_ESC_QUITS", settings.esc_quits, Some("0"));
    plan.flag("GOW_CANCEL", settings.cancel_animations, Some("0"));
    // Configurable keyboard and mouse (ps2_pad.cpp): "action=code,code;...".
    plan.text_or_remove("PC_KEYS", &keys::serialize_bindings(&keys::read_bindings(&settings.keys)));
    // Cheat menu (F2 in game): the ones active at start-up and the menu language.
    plan.text_or_remove("PC_CHEATS", &settings.cheats);
    if cheats_shortcut_is_valid(&settings.cheats_shortcut) {
        plan.insert("PC_CHEATS_SHORTCUT", settings.cheats_shortcut.trim());
    } else {
        // Also discards old incompatible bindings: the launcher only uses XInput.
        plan.remove("PC_CHEATS_SHORTCUT");
    }
    let language = if settings.language.is_empty() { "en" } else { &settings.language };
    plan.insert("PC_LANGUAGE", language);

    // Window.
    let fullscreen = settings.display_mode == "fullscreen";
    plan.insert("PC_SCREEN", if fullscreen { "fullscreen" } else { "window" });
    plan.insert("PC_WIDTH", settings.width_px.to_string());
    plan.insert("PC_HEIGHT", settings.height_px.to_string());
    plan.insert("PC_TITLE", "God of War");
    plan.insert("PC_PANEL", "0"); // the debug panel opens with F1
    if let Some(icon) = ctx.icon.as_ref().filter(|icon| icon.exists()) {
        plan.insert("PC_ICON", icon.to_string_lossy());
    }

    // Image: the widescreen switch (PCSX2's "Widescreen 16:9" patch: the camera is widened to the sides, Hor+) and the
    // geometry of the picture.
    let aspect = settings.aspect_mode.as_str();
    // A 4:3 aspect has nothing to widen, so it is the same as the switch off.
    let widescreen = settings.widescreen && !aspect.is_empty() && aspect != "4:3";
    if widescreen {
        plan.insert("PC_WIDESCREEN", aspect);
    } else {
        plan.remove("PC_WIDESCREEN");
    }
    // An explicit aspect must keep its geometry on any monitor. Sending "auto" here made 4:3 fall into the game's
    // internal widescreen (16:9) in windowed mode. "stretch" is only allowed without a fixed aspect. With the switch off
    // the game draws its original 4:3 picture, which is presented at 4:3 (side bars on a wide window) rather than
    // stretched.
    let presentation_aspect = if widescreen {
        aspect
    } else if aspect.is_empty() {
        if settings.stretch {
            "stretch"
        } else {
            "auto"
        }
    } else {
        "4:3"
    };
    plan.insert("PC_ASPECT", presentation_aspect);
    let filter = if settings.filter.is_empty() { "fsr" } else { settings.filter.as_str() };
    plan.insert("PC_FILTER", filter);
    // Sharpness 0..1 in the interface -> RCAS steps (2 = soft, 0 = maximum).
    let sharpness = settings.sharpness.clamp(0.0, 1.0);
    plan.insert("PC_SHARPNESS", fixed(2.0 * (1.0 - sharpness), 2));
    plan.flag("PC_FXAA", settings.fxaa, None);
    // Shade Boost (gs_host_present.cpp), 0..100: only a value that changes the picture is sent, so 50 leaves the shader
    // exactly as it was.
    for (name, value) in [
        ("PC_BRIGHTNESS", settings.brightness),
        ("PC_CONTRAST", settings.contrast),
        ("PC_GAMMA", settings.gamma),
        ("PC_SATURATION", settings.saturation),
    ] {
        let value = value.clamp(0, 100);
        if value == crate::settings::IMAGE_NEUTRAL {
            plan.remove(name);
        } else {
            plan.insert(name, value.to_string());
        }
    }
    // Forced progressive 480p (gow_video.cpp): on by default (sharp image); off = the game's interlaced mode.
    plan.flag("GOW_PROGRESSIVE", settings.progressive480p, Some("0"));
    // Waffle's runtime scaling (gs_host_present.cpp). "fsr" = bicubic with enhancement (GOW_SHARPEN from 0 to 0.5
    // with the sharpness); "bilinear" and "pixel" = bilinear stretch with no enhancement (soft image).
    if filter == "fsr" {
        plan.insert("GOW_SCALER", "sharp");
        plan.insert("GOW_SHARPEN", fixed(0.5 * sharpness, 2));
    } else {
        plan.insert("GOW_SCALER", "bilinear");
        plan.insert("GOW_SHARPEN", "0");
    }
    // Internal GS resolution, 1..8 (recommended 2x; from 3x up it needs a strong GPU, and the runtime keeps the render
    // targets within the GPU's texture limit).
    // Below 1x (down to a quarter) the game is drawn smaller than the PS2 and stretched to the window: integrated GPUs.
    let scale = ctx.scale.clamp(crate::settings::MIN_SCALE, crate::settings::MAX_SCALE);
    let digits = if scale < 1.0 { 2 } else { 1 };
    plan.insert("PS2X_GS_SCALE", fixed(scale, digits));
    plan.insert("PS2X_GS_HW_SCALE", fixed(scale, digits)); // Waffle's renderer reads the scale from here
    plan.insert("PC_FALLBACK_SMOOTHNESS", "0");
    // The shared queue stays non-blocking at high scale; the value can be overridden for diagnostics.
    keep_inherited(&mut plan, "GOW_PRESENT_BLOCKING", "0");
    keep_inherited(&mut plan, "GOW_SHARED_WAIT_US", "0");
    // FPS limit: the game's real frame rate. 60 is the PS2's own; 120 runs the game logic at that rate (PC_LOGIC_FPS,
    // src/game/gow_logic_rate.cpp) with the game kept at real-time speed. Any other value in settings.json snaps to the
    // nearest limit. At 60 the window refreshes at 60: only then does the runtime pace it by the game's own VBlank
    // (hostSyncToVBlank in ps2_runtime.cpp), the smoothest mode measured; a 120 Hz window on a 144 Hz display ran on its
    // own timer instead and showed 46 to 54 new pictures per second while the game made 60. At 120 the window refreshes at
    // the display's rate, at most MAX_FPS.
    let fps = fps_limit(settings.fps_limit);
    plan.insert("PC_GAME_FPS", "60");
    let window_fps = if fps > 60 { ctx.refresh_hz.clamp(60, MAX_FPS) } else { 60 };
    plan.insert("PC_WINDOW_FPS", window_fps.to_string());
    if fps > 60 {
        plan.insert("PC_LOGIC_FPS", fps.to_string());
    } else {
        plan.remove("PC_LOGIC_FPS");
    }
    // The window advances to a new GS frame at most once per 80 % of a frame at the limit (13.3 ms at 60): a burst of
    // completed frames is spread over the next refreshes instead of being shown faster than real time. It must stay below
    // two refreshes of a fast display (13.9 ms at 144 Hz), or 60 FPS would be shown at 48.
    keep_inherited(&mut plan, "GOW_PRESENT_MIN_INTERVAL_US", &(800_000 / fps).to_string());
    plan.flag("PC_VSYNC", settings.vsync, None);
    plan.flag("PC_SHOW_FPS", settings.show_fps, None);
    // Each part is sent as 0 or 1: the runtime makes an unset part follow PC_SHOW_FPS (what older launchers relied on).
    plan.flag("PC_SHOW_GPU", settings.show_gpu, Some("0"));
    plan.flag("PC_SHOW_CPU", settings.show_cpu, Some("0"));
    plan.flag("PC_SHOW_RAM", settings.show_ram, Some("0"));
    // Early FINISH is on in the runtime by default; "0" turns it off.
    plan.flag("GOW_GS_EARLY_FINISH", settings.early_finish, Some("0"));
    // Effect switches: on in the runtime by default, "0" masks the effect (src/lib/gs/gs_frontend.cpp).
    plan.flag("GOW_FX_SHADOWS", settings.fx_shadows, Some("0"));
    plan.flag("GOW_FX_ADDITIVE", settings.fx_additive, Some("0"));
    plan.flag("GOW_FX_FILTER", settings.fx_filter, Some("0"));

    // Files. The runtime reads the disc from the ISO (without an extracted PART2.PAK there is no complete disc in the
    // directory); without a chosen ISO the first one next to the ELF is used.
    let elf_directory = Path::new(&settings.elf).parent().unwrap_or_else(|| Path::new(""));
    let mut iso = settings.iso.clone();
    if iso.is_empty() && !elf_directory.join(".disc_layout").exists() {
        iso = game::first_iso(elf_directory).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    }
    plan.text_or_remove("GOW_ISO", &iso);

    // Progressive optimizer: the game records each new VU1 microprogram in this directory.
    if !ctx.vu1_capture.is_empty() {
        plan.insert("GOW_VU1_CAPTURE", &ctx.vu1_capture);
    }

    // RetroAchievements. The password lives in the system credential store and is only added to this ephemeral
    // environment.
    let user = settings.retro_achievements_user.trim();
    let password = ctx.retro_achievements_password.as_deref().filter(|_| settings.retro_achievements_enabled);
    let has_source = !iso.is_empty() || Path::new(&settings.elf).is_file();
    // GOW-Port: only the original USA executable. A translation or any other edition the port accepts plays with the same
    // code, but RetroAchievements identifies the game by the original disc: they stay off (the tab says why).
    let original = std::fs::read(&settings.elf).is_ok_and(|bytes| game::elf_crc(&bytes) == game::CRC_GOD_OF_WAR);
    let enabled = settings.retro_achievements_enabled
        && !user.is_empty()
        && password.is_some_and(|p| !p.is_empty())
        && has_source
        && original;
    plan.insert("PC_RA_ENABLED", if enabled { "1" } else { "0" });
    if enabled {
        let hardcore = settings.retro_achievements_hardcore;
        plan.insert("PC_RA_USERNAME", user);
        plan.insert("PC_RA_PASSWORD", password.unwrap_or_default());
        plan.insert("PC_RA_HARDCORE", if hardcore { "1" } else { "0" });
        plan.insert("PC_RA_NOTIFICATIONS", if settings.retro_achievements_notifications { "1" } else { "0" });
        // Hardcore rules: no cheat may be enabled in the session that counts for ranking. The menu stays available
        // but starts without changes.
        if hardcore {
            plan.remove("PC_CHEATS");
        }
    } else {
        for name in ["USERNAME", "PASSWORD", "HARDCORE", "NOTIFICATIONS"] {
            plan.remove(&format!("PC_RA_{name}"));
        }
    }
    keep_inherited(&mut plan, "PS2X_GS_SHADER_CACHE_DIR", &ctx.shader_cache.to_string_lossy());
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _directory: tempfile::TempDir,
        settings: Settings,
        shader_cache: PathBuf,
    }

    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let game_dir = directory.path().join("game");
        std::fs::create_dir_all(&game_dir).unwrap();
        let elf = game_dir.join("SCUS_973.99");
        std::fs::write(&elf, b"x").unwrap();
        // What a first run really looks like: the defaults after the launcher's migrations.
        let mut settings = Settings {
            elf: elf.to_string_lossy().into_owned(),
            executable: directory.path().join(game::EXECUTABLE_NAME).to_string_lossy().into_owned(),
            ..Settings::default()
        };
        settings.migrate(false, &crate::settings::DisplayInfo::default());
        let shader_cache = directory.path().join("shaders");
        Fixture { _directory: directory, settings, shader_cache }
    }

    fn plan(fixture: &Fixture, scale: f64) -> EnvironmentPlan {
        plan_with(fixture, scale, |_| None)
    }

    fn plan_with(fixture: &Fixture, scale: f64, inherited: impl Fn(&str) -> Option<String>) -> EnvironmentPlan {
        plan_at(fixture, scale, 144, inherited)
    }

    /// The plan on a display refreshing at `refresh_hz` (the other helpers use 144 Hz).
    fn plan_at(
        fixture: &Fixture,
        scale: f64,
        refresh_hz: u32,
        inherited: impl Fn(&str) -> Option<String>,
    ) -> EnvironmentPlan {
        let ctx = LaunchContext {
            settings: &fixture.settings,
            scale,
            vu1_capture: String::new(),
            gpu: "auto".into(),
            icon: None,
            shader_cache: fixture.shader_cache.clone(),
            pad_devices_file: None,
            retro_achievements_password: None,
            refresh_hz,
        };
        build_environment(&ctx, inherited)
    }

    fn value<'a>(plan: &'a EnvironmentPlan, name: &str) -> Option<&'a str> {
        plan.set.get(name).map(String::as_str)
    }

    #[test]
    fn base_environment_respects_what_the_user_already_set() {
        let fixture = fixture();
        let defaults = plan(&fixture, 2.0);
        assert_eq!(value(&defaults, "PS2X_GS_WORKER"), Some("1"));
        assert_eq!(value(&defaults, "GOW_PERF_FRAME_PC"), Some("0x001837B8"));
        assert_eq!(value(&defaults, "GOW_NATIVE_CHEATS_MENU"), Some("1"));
        assert_eq!(value(&defaults, "GOW_PRESENT_BLOCKING"), Some("0"));
        // the user can turn the Port Options entry off from the environment
        let off = plan_with(&fixture, 2.0, |name| (name == "GOW_NATIVE_CHEATS_MENU").then(|| "0".to_string()));
        assert_eq!(value(&off, "GOW_NATIVE_CHEATS_MENU"), None);
        let overridden = plan_with(&fixture, 2.0, |name| (name == "PS2X_GS_WORKER").then(|| "0".to_string()));
        assert_eq!(value(&overridden, "PS2X_GS_WORKER"), None);
        assert_eq!(overridden.resolve("PS2X_GS_WORKER", |_| Some("0".into())), Some("0".into()));
    }

    #[test]
    fn default_settings_golden_environment() {
        let fixture = fixture();
        let plan = plan(&fixture, 2.0);
        let expected: &[(&str, &str)] = &[
            ("PC_ASPECT", "16:9"),
            ("PC_DMC5_CONTROLS", "0"),
            ("PC_ESC_QUITS", "0"),
            ("PC_FALLBACK_SMOOTHNESS", "0"),
            ("PC_FILTER", "bilinear"),
            ("PC_FXAA", "1"),
            ("PC_GAME_FPS", "60"),
            ("PC_HEIGHT", "800"),
            ("PC_LANGUAGE", "en"),
            ("PC_PANEL", "0"),
            ("PC_RA_ENABLED", "0"),
            ("PC_SCREEN", "fullscreen"),
            ("PC_SHOW_CPU", "0"),
            ("PC_SHOW_GPU", "0"),
            ("PC_SHOW_RAM", "0"),
            ("PC_SHARPNESS", "0.40"),
            ("PC_TITLE", "God of War"),
            ("PC_WIDESCREEN", "16:9"),
            ("PC_WIDTH", "1280"),
            ("PC_WINDOW_FPS", "60"), // at the 60 limit the window follows the game's VBlank
            ("GOW_AUDIO_DEVICE", "default"),
            ("GOW_CANCEL", "1"),
            ("GOW_FX_ADDITIVE", "1"),
            ("GOW_FX_FILTER", "1"),
            ("GOW_FX_SHADOWS", "1"),
            ("GOW_GS_EARLY_FINISH", "1"),
            ("GOW_GS_FINISH_ASYNC", "1"),
            ("GOW_NATIVE_CHEATS_MENU", "1"),
            ("GOW_PERF_DIAG", "1"),
            ("GOW_PERF_FRAME_PC", "0x001837B8"),
            ("GOW_PRESENT_BLOCKING", "0"),
            ("GOW_PRESENT_MIN_INTERVAL_US", "13333"),
            ("GOW_PROGRESSIVE", "1"),
            ("GOW_SCALER", "bilinear"),
            ("GOW_SHARED_WAIT_US", "0"),
            ("GOW_SHARPEN", "0"),
            ("PS2X_GS_HW_SCALE", "2.0"),
            ("PS2X_GS_SCALE", "2.0"),
            ("PS2X_GS_WORKER", "1"),
        ];
        let mut actual: Vec<(String, String)> = plan
            .set
            .iter()
            .filter(|(name, _)| name.as_str() != "PC_KEYS" && name.as_str() != "PS2X_GS_SHADER_CACHE_DIR")
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        actual.sort();
        let mut expected: Vec<(String, String)> =
            expected.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
        expected.sort();
        assert_eq!(actual, expected);
        for name in [
            "GOW_SKIP_FMV",
            "GOW_FAST_BOOT",
            "PC_VSYNC",
            "PC_SHOW_FPS",
            "PC_CHEATS",
            "GOW_ISO",
            "PC_PAD_FACE_MAP",
            "PC_PAD_MAP",
            "GOW_PAD_DEVICE",
            "GOW_PAD_LIST_FILE",
            "GOW_PAD_SDL_MAPPING",
            "PC_BRIGHTNESS",
            "PC_CONTRAST",
            "PC_GAMMA",
            "PC_SATURATION",
        ] {
            assert!(plan.remove.contains(name), "{name} should be removed");
        }
        assert!(value(&plan, "PC_KEYS").unwrap().starts_with("ls_up=87;"));
    }

    #[test]
    fn the_chosen_controller_is_passed_as_its_guid_and_the_list_goes_to_our_file() {
        let mut fixture = fixture();
        let guid = "0300dddd5e040000a102000000010000";
        // Nothing chosen: no GOW_PAD_DEVICE (the game picks the pad itself).
        assert!(plan(&fixture, 2.0).remove.contains("GOW_PAD_DEVICE"));
        fixture.settings.pad_device = guid.into();
        assert_eq!(value(&plan(&fixture, 2.0), "GOW_PAD_DEVICE"), Some(guid));
        // Anything that is not a GLFW GUID is not passed (a hand-edited settings file, a name in the wrong field).
        for invalid in ["Generic X-Box pad", "0300dddd", "0300dddd5e040000a102000000010000,a:b0", ""] {
            fixture.settings.pad_device = invalid.into();
            assert_eq!(value(&plan(&fixture, 2.0), "GOW_PAD_DEVICE"), None, "{invalid:?}");
        }
        // The list file is where the launcher reads it from.
        let ctx = LaunchContext {
            settings: &fixture.settings,
            scale: 2.0,
            vu1_capture: String::new(),
            gpu: "auto".into(),
            icon: None,
            shader_cache: fixture.shader_cache.clone(),
            pad_devices_file: Some(PathBuf::from("/data/pads/devices.json")),
            retro_achievements_password: None,
            refresh_hz: 144,
        };
        let plan = build_environment(&ctx, |_| None);
        assert_eq!(value(&plan, "GOW_PAD_LIST_FILE"), Some("/data/pads/devices.json"));
    }

    #[test]
    fn the_hand_made_sdl_mapping_needs_the_device_and_valid_fields() {
        let mut fixture = fixture();
        let guid = "0300dddd5e040000a102000000010000";
        fixture.settings.pad_sdl_map = "a:b0,start:b7,dpup:h0.1,leftx:a0".into();
        // Fields without a chosen device go nowhere.
        assert!(plan(&fixture, 2.0).remove.contains("GOW_PAD_SDL_MAPPING"));
        fixture.settings.pad_device = guid.into();
        fixture.settings.pad_device_name = "Generic X-Box pad".into();
        assert_eq!(
            value(&plan(&fixture, 2.0), "GOW_PAD_SDL_MAPPING"),
            Some("0300dddd5e040000a102000000010000,Generic X-Box pad,a:b0,start:b7,dpup:h0.1,leftx:a0")
        );
        // Without a start button the game could not be started: nothing is passed.
        fixture.settings.pad_sdl_map = "a:b0".into();
        assert_eq!(value(&plan(&fixture, 2.0), "GOW_PAD_SDL_MAPPING"), None);
        fixture.settings.pad_sdl_map = String::new();
        assert_eq!(value(&plan(&fixture, 2.0), "GOW_PAD_SDL_MAPPING"), None);
    }

    #[test]
    fn the_button_map_is_passed_only_when_it_is_valid() {
        let mut fixture = fixture();
        let valid = "a=cross;b=circle;start=start";
        fixture.settings.pad_map = valid.into();
        assert_eq!(value(&plan(&fixture, 2.0), "PC_PAD_MAP"), Some(valid));
        // Invalid (no start), empty and unknown names: the variable is removed so the game keeps the presets.
        for invalid in ["a=cross;b=circle", "", "a=jump;start=start", "a=cross;a=circle;start=start"] {
            fixture.settings.pad_map = invalid.into();
            let plan = plan(&fixture, 2.0);
            assert_eq!(value(&plan, "PC_PAD_MAP"), None, "{invalid:?}");
            assert!(plan.remove.contains("PC_PAD_MAP"), "{invalid:?}");
        }
        // The face map and the shoulder preset are still passed next to it: the game decides which one wins.
        fixture.settings.pad_map = valid.into();
        fixture.settings.pad_buttons = "cross,triangle,circle,square".into();
        let plan = plan(&fixture, 2.0);
        assert_eq!(value(&plan, "PC_PAD_FACE_MAP"), Some("cross,triangle,circle,square"));
    }

    #[test]
    fn custom_textures_point_the_runner_at_the_replacements_folder() {
        let mut fixture = fixture();
        let off = plan(&fixture, 2.0);
        assert!(off.remove.contains("GOW_TEXTURE_REPLACE") && value(&off, "GOW_TEXTURE_REPLACE").is_none());
        fixture.settings.custom_textures = true;
        let on = plan(&fixture, 2.0);
        let expected =
            Path::new(&fixture.settings.elf).parent().unwrap().parent().unwrap().join("textures").join("replacements");
        assert_eq!(value(&on, "GOW_TEXTURE_REPLACE"), Some(expected.to_string_lossy().as_ref()));
        // Asynchronous loading is the default; turning it off asks the runner to wait for each file.
        assert!(value(&on, "GOW_TEXTURE_REPLACE_SYNC").is_none() && on.remove.contains("GOW_TEXTURE_REPLACE_SYNC"));
        fixture.settings.async_textures = false;
        assert_eq!(value(&plan(&fixture, 2.0), "GOW_TEXTURE_REPLACE_SYNC"), Some("1"));
        fixture.settings.async_textures = true;
        // Without an ELF there is no installation to look in.
        fixture.settings.elf.clear();
        assert!(plan(&fixture, 2.0).remove.contains("GOW_TEXTURE_REPLACE"));
    }

    #[test]
    fn each_part_of_the_osd_has_its_own_variable() {
        let mut fixture = fixture();
        fixture.settings.show_cpu = true;
        fixture.settings.show_ram = true;
        let plan = plan(&fixture, 2.0);
        assert_eq!((value(&plan, "PC_SHOW_CPU"), value(&plan, "PC_SHOW_RAM")), (Some("1"), Some("1")));
        assert!(plan.remove.contains("PC_SHOW_FPS"));
        // A part turned off is sent as 0, so the runtime does not make it follow PC_SHOW_FPS.
        assert_eq!(value(&plan, "PC_SHOW_GPU"), Some("0"));
    }

    #[test]
    fn early_finish_is_on_by_default_and_can_be_turned_off() {
        let mut fixture = fixture();
        assert_eq!(value(&plan(&fixture, 2.0), "GOW_GS_EARLY_FINISH"), Some("1"));
        fixture.settings.early_finish = false;
        assert_eq!(value(&plan(&fixture, 2.0), "GOW_GS_EARLY_FINISH"), Some("0"));
    }

    #[test]
    fn image_adjustment_is_sent_only_when_it_changes_the_picture() {
        let mut fixture = fixture();
        fixture.settings.brightness = 65;
        fixture.settings.gamma = 140; // out of range in a hand-edited file
        fixture.settings.saturation = -3;
        let plan = plan(&fixture, 2.0);
        assert_eq!(value(&plan, "PC_BRIGHTNESS"), Some("65"));
        assert_eq!(value(&plan, "PC_GAMMA"), Some("100"));
        assert_eq!(value(&plan, "PC_SATURATION"), Some("0"));
        assert!(value(&plan, "PC_CONTRAST").is_none() && plan.remove.contains("PC_CONTRAST"));
    }

    #[test]
    fn fsr_filter_sets_the_sharpening() {
        let mut fixture = fixture();
        fixture.settings.filter = "fsr".into();
        fixture.settings.sharpness = 0.5;
        let plan = plan(&fixture, 2.0);
        assert_eq!(value(&plan, "GOW_SCALER"), Some("sharp"));
        assert_eq!(value(&plan, "GOW_SHARPEN"), Some("0.25"));
        assert_eq!(value(&plan, "PC_SHARPNESS"), Some("1.00"));
    }

    #[test]
    fn four_by_three_has_no_widescreen_but_keeps_its_aspect() {
        let mut fixture = fixture();
        fixture.settings.aspect_mode = "4:3".into();
        let plan = plan(&fixture, 2.0);
        assert!(plan.remove.contains("PC_WIDESCREEN"));
        assert_eq!(value(&plan, "PC_ASPECT"), Some("4:3"));
        fixture.settings.aspect_mode.clear();
        fixture.settings.stretch = true;
        assert_eq!(value(&super::tests::plan(&fixture, 2.0), "PC_ASPECT"), Some("stretch"));
        fixture.settings.stretch = false;
        assert_eq!(value(&super::tests::plan(&fixture, 2.0), "PC_ASPECT"), Some("auto"));
    }

    #[test]
    fn the_widescreen_switch_is_what_enables_the_patch() {
        let mut fixture = fixture();
        assert_eq!(value(&plan(&fixture, 2.0), "PC_WIDESCREEN"), Some("16:9"));
        fixture.settings.widescreen = false;
        let off = plan(&fixture, 2.0);
        assert!(off.remove.contains("PC_WIDESCREEN"));
        // The original 4:3 picture keeps its geometry instead of being stretched to the 16:9 window.
        assert_eq!(value(&off, "PC_ASPECT"), Some("4:3"));
        fixture.settings.widescreen = true;
        fixture.settings.aspect_mode = "21:9".into();
        assert_eq!(value(&plan(&fixture, 2.0), "PC_WIDESCREEN"), Some("21:9"));
        fixture.settings.aspect_mode = "32:9".into();
        let super_wide = plan(&fixture, 2.0);
        assert_eq!(value(&super_wide, "PC_WIDESCREEN"), Some("32:9"));
        assert_eq!(value(&super_wide, "PC_ASPECT"), Some("32:9"));
        // With the switch off, 32:9 is not presented: the original 4:3 picture keeps its geometry.
        fixture.settings.widescreen = false;
        let off = plan(&fixture, 2.0);
        assert!(off.remove.contains("PC_WIDESCREEN"));
        assert_eq!(value(&off, "PC_ASPECT"), Some("4:3"));
        fixture.settings.widescreen = true;
        // A 4:3 aspect has nothing to widen: the switch is effectively off there.
        fixture.settings.aspect_mode = "4:3".into();
        assert!(plan(&fixture, 2.0).remove.contains("PC_WIDESCREEN"));
    }

    #[test]
    fn the_fps_limit_sets_the_logic_rate_and_the_window_follows_the_display() {
        let mut fixture = fixture();
        fixture.settings.fps_limit = 120;
        fixture.settings.vsync = true;
        let two = plan(&fixture, 2.0);
        assert_eq!(value(&two, "PC_LOGIC_FPS"), Some("120"));
        assert_eq!(value(&two, "PC_WINDOW_FPS"), Some("120")); // the test display refreshes at 144 Hz: capped at MAX_FPS
        assert_eq!(value(&two, "GOW_PRESENT_MIN_INTERVAL_US"), Some("6666"));
        assert_eq!(value(&two, "PC_VSYNC"), Some("1"));
        assert_eq!(value(&plan(&fixture, 3.0), "PC_LOGIC_FPS"), Some("120"));
        // 60 is the game's own logic rate: the experimental switch is removed, not set to 60.
        fixture.settings.fps_limit = 60;
        assert!(plan(&fixture, 2.0).remove.contains("PC_LOGIC_FPS"));
        assert_eq!(value(&plan(&fixture, 2.0), "PC_WINDOW_FPS"), Some("60")); // paced by the game's VBlank, even at 144 Hz
        assert_eq!(value(&plan(&fixture, 3.0), "GOW_PRESENT_MIN_INTERVAL_US"), Some("13333"));
        fixture.settings.fps_limit = 240; // an old setting: never above the cap
        assert_eq!(value(&plan(&fixture, 2.0), "PC_LOGIC_FPS"), Some("120"));
        assert_eq!(value(&plan(&fixture, 7.0), "PS2X_GS_SCALE"), Some("7.0"));
        assert_eq!(value(&plan(&fixture, 10.0), "PS2X_GS_SCALE"), Some("8.0"));
    }

    #[test]
    fn stored_fps_values_snap_to_60_or_120() {
        for (stored, expected) in [
            (0, 60),
            (30, 60),
            (60, 60),
            (90, 60),
            (120, 120),
            (144, 120),
            (165, 120),
            (200, 120),
            (240, 120),
            (500, 120),
        ] {
            assert_eq!(fps_limit(stored), expected, "stored {stored}");
        }
    }

    #[test]
    fn the_window_refresh_is_kept_between_60_and_120() {
        let mut fixture = fixture();
        fixture.settings.fps_limit = 120;
        for (hz, expected) in [(30, "60"), (60, "60"), (100, "100"), (144, "120"), (165, "120"), (500, "120")] {
            assert_eq!(value(&plan_at(&fixture, 2.0, hz, |_| None), "PC_WINDOW_FPS"), Some(expected), "{hz} Hz");
        }
    }

    #[test]
    fn the_cheat_shortcut_is_validated() {
        for good in ["6", "6+9", " 17 ", "1+17"] {
            assert!(cheats_shortcut_is_valid(good), "{good}");
        }
        for bad in ["", "0", "18", "6+9+3", "a", "6+", "+6", "-1", "6++9"] {
            assert!(!cheats_shortcut_is_valid(bad), "{bad}");
        }
        let mut fixture = fixture();
        fixture.settings.cheats_shortcut = " 6+9 ".into();
        assert_eq!(value(&plan(&fixture, 2.0), "PC_CHEATS_SHORTCUT"), Some("6+9"));
        fixture.settings.cheats_shortcut = "99".into();
        assert!(plan(&fixture, 2.0).remove.contains("PC_CHEATS_SHORTCUT"));
    }

    #[test]
    fn iso_is_the_chosen_one_or_the_first_next_to_the_elf() {
        let mut fixture = fixture();
        let game_dir = Path::new(&fixture.settings.elf).parent().unwrap().to_path_buf();
        std::fs::write(game_dir.join("b.iso"), b"x").unwrap();
        std::fs::write(game_dir.join("a.iso"), b"x").unwrap();
        assert!(value(&plan(&fixture, 2.0), "GOW_ISO").unwrap().ends_with("a.iso"));
        std::fs::write(game_dir.join(".disc_layout"), b"x").unwrap(); // extracted disc: no ISO guess
        assert!(plan(&fixture, 2.0).remove.contains("GOW_ISO"));
        fixture.settings.iso = "C:/chosen.iso".into();
        assert_eq!(value(&plan(&fixture, 2.0), "GOW_ISO"), Some("C:/chosen.iso"));
    }

    fn retro_plan(fixture: &Fixture, password: Option<&str>) -> EnvironmentPlan {
        let ctx = LaunchContext {
            settings: &fixture.settings,
            scale: 2.0,
            vu1_capture: "C:/vu1".into(),
            gpu: "auto".into(),
            icon: None,
            shader_cache: fixture.shader_cache.clone(),
            pad_devices_file: None,
            retro_achievements_password: password.map(str::to_string),
            refresh_hz: 144,
        };
        build_environment(&ctx, |_| None)
    }

    #[test]
    fn retro_achievements_need_everything_and_hardcore_clears_the_cheats() {
        let mut fixture = fixture();
        fixture.settings.retro_achievements_enabled = true;
        fixture.settings.retro_achievements_user = " player ".into();
        fixture.settings.cheats = "god_mode".into();

        // GOW-Port: another edition or a translation (its ELF is not the original): achievements stay off.
        assert_eq!(value(&retro_plan(&fixture, Some("secret")), "PC_RA_ENABLED"), Some("0"));
        // the original executable: one word holding the CRC is enough for game::elf_crc
        std::fs::write(&fixture.settings.elf, crate::game::CRC_GOD_OF_WAR.to_le_bytes()).unwrap();

        assert_eq!(value(&retro_plan(&fixture, None), "PC_RA_ENABLED"), Some("0"));
        assert!(retro_plan(&fixture, None).remove.contains("PC_RA_PASSWORD"));

        let on = retro_plan(&fixture, Some("secret"));
        assert_eq!(value(&on, "PC_RA_ENABLED"), Some("1"));
        assert_eq!(value(&on, "PC_RA_USERNAME"), Some("player"));
        assert_eq!(value(&on, "PC_RA_PASSWORD"), Some("secret"));
        assert_eq!(value(&on, "PC_RA_HARDCORE"), Some("0"));
        assert_eq!(value(&on, "PC_RA_NOTIFICATIONS"), Some("1"));
        assert_eq!(value(&on, "PC_CHEATS"), Some("god_mode"));
        assert_eq!(value(&on, "GOW_VU1_CAPTURE"), Some("C:/vu1"));

        fixture.settings.retro_achievements_hardcore = true;
        let hardcore = retro_plan(&fixture, Some("secret"));
        assert!(hardcore.remove.contains("PC_CHEATS"));
        assert_eq!(value(&hardcore, "PC_CHEATS"), None);

        // Disabled in the settings: a stored password is never used.
        fixture.settings.retro_achievements_enabled = false;
        assert_eq!(value(&retro_plan(&fixture, Some("secret")), "PC_RA_ENABLED"), Some("0"));
    }

    #[test]
    fn validation_reports_the_first_problem() {
        let mut fixture = fixture();
        let error = validate(&fixture.settings).unwrap_err();
        assert_eq!(error.key(), "errorExecutable");
        std::fs::write(&fixture.settings.executable, b"x").unwrap();
        // On Linux a runner has to be executable to count as one (on Windows the extension decides).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fixture.settings.executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // a one-byte file is not an ELF: valid is false -> mismatch
        assert_eq!(validate(&fixture.settings).unwrap_err().key(), "errorElfMismatch");
        fixture.settings.elf = "/missing".into();
        assert_eq!(validate(&fixture.settings).unwrap_err().key(), "errorElf");
    }
}
