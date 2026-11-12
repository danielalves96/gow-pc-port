//! Launcher settings, stored as JSON. Field names and defaults are those of the former QML `Settings` block
//! (`launcher/qml/Main.qml` in the removed Qt launcher; see git history), and `migrate` replays its `settingsVersion` steps.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 14;

/// Highest internal GS scale the launcher offers (the runtime also keeps it within the GPU's texture limit).
pub const MAX_SCALE: f64 = 8.0;

/// Lowest internal GS scale the launcher offers: a quarter of the PS2's resolution, for integrated GPUs.
pub const MIN_SCALE: f64 = 0.25;

/// Value of the four Shade Boost settings that leaves the picture as the game draws it.
pub const IMAGE_NEUTRAL: i32 = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: String,
    pub gpu: String,
    pub display_mode: String,
    pub width_px: i32,
    pub height_px: i32,
    pub aspect_mode: String,
    /// Widescreen switch: PCSX2's "Widescreen 16:9" patch (Hor+ camera at `aspect_mode`, `PC_WIDESCREEN`). Off keeps the
    /// game's original 4:3 picture.
    pub widescreen: bool,
    pub stretch: bool,
    pub filter: String,
    pub sharpness: f64,
    pub fxaa: bool,
    /// Shade Boost (PCSX2-style image adjustment on the final picture), 0..100 with 50 = no change: `PC_BRIGHTNESS`,
    /// `PC_CONTRAST`, `PC_GAMMA` and `PC_SATURATION`. A file written before they existed reads 50 (`serde(default)`).
    pub brightness: i32,
    pub contrast: i32,
    pub gamma: i32,
    pub saturation: i32,
    pub progressive480p: bool,
    /// "auto" or "0.25".."8" (`PS2X_GS_SCALE`). Below 1 the game is drawn smaller than the PS2 and stretched (integrated GPUs).
    pub internal_resolution: String,
    pub fps_limit: i32,
    pub vsync: bool,
    /// In-game OSD, one switch per part: the FPS counter (`PC_SHOW_FPS`), GPU usage, temperature and video memory
    /// (`PC_SHOW_GPU`), CPU usage and clock (`PC_SHOW_CPU`) and the game's RAM (`PC_SHOW_RAM`).
    pub show_fps: bool,
    pub show_gpu: bool,
    pub show_cpu: bool,
    pub show_ram: bool,
    /// Early FINISH (`GOW_GS_EARLY_FINISH`): the game prepares the next frame while the current one is still drawn.
    pub early_finish: bool,
    /// Effect switches for weak GPUs (`GOW_FX_SHADOWS`, `GOW_FX_ADDITIVE`, `GOW_FX_FILTER`): character shadows, additive 3D
    /// effects (fire, glows, particles) and bilinear texture filtering. On is the game as it is drawn.
    pub fx_shadows: bool,
    pub fx_additive: bool,
    pub fx_filter: bool,
    pub elf: String,
    pub iso: String,
    pub executable: String,
    pub skip_videos: bool,
    pub fast_boot: bool,
    /// Draw the PCSX2-format texture replacements of `<installation>/textures/replacements` (`GOW_TEXTURE_REPLACE`).
    pub custom_textures: bool,
    /// Load the replacements in the background (the game's own texture is drawn until each one is ready); off waits for
    /// every file (`GOW_TEXTURE_REPLACE_SYNC`).
    pub async_textures: bool,
    pub settings_version: u32,
    pub dmc5_controls: bool,
    /// A,B,X,Y -> cross,circle,square,triangle; empty = preset.
    pub pad_buttons: String,
    /// `source=target;...` for every gamepad button (`PC_PAD_MAP`, see `padmap.rs`); empty = the preset and the face buttons above.
    pub pad_map: String,
    /// GUID of the joystick the game reads (`GOW_PAD_DEVICE`); empty = the game picks the pad itself.
    pub pad_device: String,
    /// Its name, kept only to show it while the device is not connected.
    pub pad_device_name: String,
    /// SDL mapping fields for that device when the system has none (`a:b0,start:b7,...`, see `controller.rs`); empty = none.
    pub pad_sdl_map: String,
    pub cancel_animations: bool,
    pub esc_quits: bool,
    pub minimize_on_play: bool,
    /// Opening the launcher starts the game without the window (issue #40; see `direct`). Shift while it opens, or
    /// `--launcher`, shows the window.
    pub skip_launcher: bool,
    pub seconds_played: u64,
    pub last_session: u64,
    /// Key map in JSON (empty = default scheme).
    pub keys: String,
    /// Cheats active at start-up, comma-separated.
    pub cheats: String,
    /// raylib buttons ("6" or "6+9") that open the cheat menu.
    pub cheats_shortcut: String,
    pub retro_achievements_enabled: bool,
    pub retro_achievements_user: String,
    pub retro_achievements_hardcore: bool,
    pub retro_achievements_notifications: bool,
    pub optimization_destination: String,
    pub optimization_enabled: bool,
    pub optimize_during_game: bool,
    pub optimization_compiled: u32,
    /// Download and install a newer launcher at start-up when nothing is running. Off: the launcher only shows a notice.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: String::new(),
            gpu: "auto".into(),
            display_mode: "fullscreen".into(),
            width_px: 1280,
            height_px: 800,
            aspect_mode: String::new(),
            widescreen: true,
            stretch: false,
            filter: "bilinear".into(),
            sharpness: 0.8,
            fxaa: true,
            brightness: IMAGE_NEUTRAL,
            contrast: IMAGE_NEUTRAL,
            gamma: IMAGE_NEUTRAL,
            saturation: IMAGE_NEUTRAL,
            progressive480p: true,
            internal_resolution: "2".into(),
            fps_limit: 60,
            vsync: false,
            show_fps: false,
            show_gpu: false,
            show_cpu: false,
            show_ram: false,
            early_finish: true,
            fx_shadows: true,
            fx_additive: true,
            fx_filter: true,
            elf: String::new(),
            iso: String::new(),
            executable: String::new(),
            skip_videos: true,
            fast_boot: false,
            custom_textures: false,
            async_textures: true,
            settings_version: 0,
            dmc5_controls: false,
            pad_buttons: String::new(),
            pad_map: String::new(),
            pad_device: String::new(),
            pad_device_name: String::new(),
            pad_sdl_map: String::new(),
            cancel_animations: true,
            esc_quits: false,
            minimize_on_play: true,
            skip_launcher: false,
            seconds_played: 0,
            last_session: 0,
            keys: String::new(),
            cheats: String::new(),
            cheats_shortcut: String::new(),
            retro_achievements_enabled: false,
            retro_achievements_user: String::new(),
            retro_achievements_hardcore: false,
            retro_achievements_notifications: true,
            optimization_destination: String::new(),
            optimization_enabled: true,
            // Off: a rebuild running with the game open starves it (the game stops entering its frame function and the
            // picture freezes). Rebuilds run with the game closed, and one started before the game opens is stopped.
            optimize_during_game: false,
            optimization_compiled: 0,
            auto_update: false,
        }
    }
}

/// Main display in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub width_px: u32,
    pub height_px: u32,
    pub hz: u32,
}

impl Default for DisplayInfo {
    fn default() -> Self {
        Self { width_px: 1920, height_px: 1080, hz: 60 }
    }
}

impl DisplayInfo {
    /// Closest aspect the port offers for this display.
    pub fn default_aspect_ratio(&self) -> &'static str {
        let ratio = self.width_px as f64 / self.height_px.max(1) as f64;
        // 21:9 monitors are 2.33 to 2.4 (2560x1080, 3440x1440), 32:9 ones 3.56 (3840x1080, 5120x1440).
        if ratio >= 3.0 {
            "32:9"
        } else if ratio >= 2.2 {
            "21:9"
        } else if ratio >= 1.7 {
            "16:9"
        } else if ratio >= 1.5 {
            "16:10"
        } else {
            "4:3"
        }
    }
}

impl Settings {
    /// Reads the file; a missing or unreadable file gives the defaults (the caller migrates afterwards).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Atomic write: a crash in the middle never leaves a half-written file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&temporary, path)
    }

    /// Replays the `settingsVersion` steps of the QML launcher and fills what is still empty.
    pub fn migrate(&mut self, prefers_portuguese: bool, display: &DisplayInfo) {
        if self.settings_version < 2 {
            // Fast boot came enabled and cut the opening; it is turned off once.
            self.fast_boot = false;
            self.settings_version = 2;
        }
        if self.settings_version < 3 {
            self.internal_resolution = "2".into();
            self.settings_version = 3;
        }
        if self.settings_version < 4 {
            self.settings_version = 4; // historical renderer migration
        }
        if self.settings_version < 5 {
            self.skip_videos = true;
            self.settings_version = 5;
        }
        if self.settings_version < 6 {
            // Waffle's runtime plays the videos natively, with sound.
            self.skip_videos = false;
            self.settings_version = 6;
        }
        if self.settings_version < 7 {
            // Old DMC5 preset that was enabled by default.
            self.dmc5_controls = false;
            self.pad_buttons.clear();
            self.settings_version = 7;
        }
        if self.settings_version < 9 {
            // The game keeps its logic at 60 Hz; 120 only speeds up the presentation.
            self.fps_limit = if self.fps_limit >= 120 { 120 } else { 60 };
            self.settings_version = 9;
        }
        if self.settings_version < 10 {
            // Rebuilding with the game open starves it (long freezes): it is off for everyone once. Whoever wants it back turns
            // the switch on again, now with a warning next to it.
            self.optimize_during_game = false;
            self.settings_version = 10;
        }
        if self.settings_version < 11 {
            // There is no unlimited presentation any more: the modes are 60, 120 and 144, and any other stored value (0 was
            // unlimited) snaps to the nearest one.
            self.fps_limit = match self.fps_limit {
                i32::MIN..=60 => 60,
                61..=120 => 120,
                _ => 144,
            };
            self.settings_version = 11;
        }
        if self.settings_version < 12 {
            // 120 and 144 used to speed up only the window; now they also run the game logic at that rate (experimental).
            // Nobody gets that without choosing it again.
            self.fps_limit = 60;
            self.settings_version = 12;
        }
        if self.settings_version < 13 {
            // The FPS switch used to show the whole OSD (GPU, CPU, RAM too); now each part has its own. Whoever had it on
            // keeps seeing everything.
            if self.show_fps {
                self.show_gpu = true;
                self.show_cpu = true;
                self.show_ram = true;
            }
            self.settings_version = 13;
        }
        if self.settings_version < 14 {
            // The project never runs above 120 FPS (crate::launch::MAX_FPS): the experimental 240 is gone, and whoever had it
            // gets 120.
            self.fps_limit = crate::launch::fps_limit(self.fps_limit) as i32;
            self.settings_version = 14;
        }
        if self.language != "pt-BR" && self.language != "en" {
            self.language = if prefers_portuguese { "pt-BR".into() } else { "en".into() };
        }
        if self.aspect_mode.is_empty() {
            self.aspect_mode = display.default_aspect_ratio().into();
        }
    }

    /// Internal GS scale actually requested: "auto" and anything above 8 fall back to 2x, anything below a quarter to 0.25x.
    pub fn selected_scale(&self) -> f64 {
        match self.internal_resolution.parse::<f64>() {
            Ok(n) if self.internal_resolution != "auto" && n <= MAX_SCALE => n.max(MIN_SCALE),
            _ => 2.0,
        }
    }

    /// Brightness, contrast, gamma and saturation back to "no change" (the Reset button of the image adjustment group).
    pub fn reset_image_adjustment(&mut self) {
        self.brightness = IMAGE_NEUTRAL;
        self.contrast = IMAGE_NEUTRAL;
        self.gamma = IMAGE_NEUTRAL;
        self.saturation = IMAGE_NEUTRAL;
    }

    /// Back to the defaults of the "Reset" button; paths, cheats' history, RetroAchievements and the hand-made mapping of a
    /// controller (`pad_sdl_map`) are untouched.
    pub fn reset_game_options(&mut self, display: &DisplayInfo) {
        self.gpu = "auto".into();
        self.display_mode = "fullscreen".into();
        self.width_px = 1280;
        self.height_px = if (display.width_px as f64 / display.height_px.max(1) as f64) < 1.7 { 800 } else { 720 };
        self.aspect_mode = display.default_aspect_ratio().into();
        self.widescreen = true;
        self.stretch = false;
        self.filter = "bilinear".into();
        self.sharpness = 0.8;
        self.fxaa = true;
        self.reset_image_adjustment();
        self.progressive480p = true;
        self.internal_resolution = "2".into();
        self.fx_shadows = true;
        self.fx_additive = true;
        self.fx_filter = true;
        self.fps_limit = 60;
        self.vsync = false;
        self.show_fps = false;
        self.show_gpu = false;
        self.show_cpu = false;
        self.show_ram = false;
        self.skip_videos = true;
        self.fast_boot = false;
        self.dmc5_controls = false;
        self.pad_buttons.clear();
        self.pad_map.clear();
        self.pad_device.clear();
        self.pad_device_name.clear();
        // `pad_sdl_map` stays: for a controller the system has no layout for, the mapping the player built is the only thing
        // that makes it work, which is a device fact more than a preference.
        self.cancel_animations = true;
        self.esc_quits = false;
        self.minimize_on_play = true;
        self.cheats.clear();
        self.cheats_shortcut.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_ends_at_the_current_version_with_videos_on() {
        let mut settings = Settings::default();
        settings.migrate(false, &DisplayInfo::default());
        assert_eq!(settings.settings_version, SETTINGS_VERSION);
        assert!(!settings.skip_videos); // v5 turns them off, v6 puts them back
        assert_eq!(settings.language, "en");
        assert_eq!(settings.aspect_mode, "16:9");
    }

    /// The "Balanced" graphics preset (`src/lib/presets.ts`) is what a fresh install and the Reset button give. The
    /// TypeScript table is a copy, so this pins the Rust side of it: change one of the four defaults and this fails,
    /// together with the line in `presets.test.ts` that holds the same values.
    #[test]
    fn the_balanced_preset_is_what_a_fresh_install_and_a_reset_give() {
        let balanced = (("2", "bilinear"), (0.8_f64, true));
        let fresh = Settings::default();
        assert_eq!(
            ((fresh.internal_resolution.as_str(), fresh.filter.as_str()), (fresh.sharpness, fresh.fxaa)),
            balanced
        );
        let mut reset = Settings {
            internal_resolution: "4".into(),
            filter: "fsr".into(),
            sharpness: 0.1,
            fxaa: false,
            ..Settings::default()
        };
        reset.reset_game_options(&DisplayInfo::default());
        assert_eq!(
            ((reset.internal_resolution.as_str(), reset.filter.as_str()), (reset.sharpness, reset.fxaa)),
            balanced
        );
    }

    #[test]
    fn language_follows_the_system_only_when_unsupported() {
        let mut settings = Settings { language: "es".into(), ..Settings::default() };
        settings.migrate(true, &DisplayInfo::default());
        assert_eq!(settings.language, "pt-BR");
        let mut settings = Settings { language: "en".into(), ..Settings::default() };
        settings.migrate(true, &DisplayInfo::default());
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn version_twelve_resets_the_fps_mode_once() {
        // 120/144 now also mean the experimental logic rate: an old choice goes back to 60 once.
        for stored in [30, 60, 120, 144] {
            let mut settings = Settings { settings_version: 8, fps_limit: stored, ..Settings::default() };
            settings.migrate(false, &DisplayInfo::default());
            assert_eq!(settings.fps_limit, 60, "stored {stored}");
        }
        // A 120 chosen after the migration is kept.
        let mut chosen = Settings { settings_version: 12, fps_limit: 120, ..Settings::default() };
        chosen.migrate(false, &DisplayInfo::default());
        assert_eq!(chosen.fps_limit, 120);
    }

    #[test]
    fn version_fourteen_caps_the_fps_limit_at_120() {
        for (stored, expected) in [(60, 60), (120, 120), (240, 120)] {
            let mut settings = Settings { settings_version: 13, fps_limit: stored, ..Settings::default() };
            settings.migrate(false, &DisplayInfo::default());
            assert_eq!(settings.fps_limit, expected, "stored {stored}");
        }
    }

    #[test]
    fn version_thirteen_keeps_the_whole_osd_for_whoever_had_the_fps_switch_on() {
        let mut shown = Settings { settings_version: 12, show_fps: true, ..Settings::default() };
        shown.migrate(false, &DisplayInfo::default());
        assert!(shown.show_fps && shown.show_gpu && shown.show_cpu && shown.show_ram);
        let mut hidden = Settings { settings_version: 12, ..Settings::default() };
        hidden.migrate(false, &DisplayInfo::default());
        assert!(!hidden.show_fps && !hidden.show_gpu && !hidden.show_cpu && !hidden.show_ram);
        // After the migration the parts are independent: FPS alone stays alone.
        let mut fps_only = Settings { settings_version: 13, show_fps: true, ..Settings::default() };
        fps_only.migrate(false, &DisplayInfo::default());
        assert!(fps_only.show_fps && !fps_only.show_gpu && !fps_only.show_cpu && !fps_only.show_ram);
    }

    #[test]
    fn a_current_file_is_left_alone() {
        let mut settings = Settings {
            settings_version: SETTINGS_VERSION,
            skip_videos: true,
            fast_boot: true,
            language: "pt-BR".into(),
            aspect_mode: "21:9".into(),
            ..Settings::default()
        };
        let before = settings.clone();
        settings.migrate(false, &DisplayInfo::default());
        assert_eq!(settings, before);
    }

    #[test]
    fn version_ten_turns_off_the_rebuild_during_the_game_once() {
        let mut old = Settings { settings_version: 9, optimize_during_game: true, ..Settings::default() };
        old.migrate(false, &DisplayInfo::default());
        assert!(!old.optimize_during_game);
        assert_eq!(old.settings_version, SETTINGS_VERSION);
        // Whoever turns it back on keeps it: the migration is not replayed.
        let mut chosen = Settings { settings_version: 10, optimize_during_game: true, ..Settings::default() };
        chosen.migrate(false, &DisplayInfo::default());
        assert!(chosen.optimize_during_game);
    }

    #[test]
    fn aspect_follows_the_display() {
        let wide = DisplayInfo { width_px: 3440, height_px: 1440, hz: 60 };
        let laptop = DisplayInfo { width_px: 1920, height_px: 1200, hz: 60 };
        let old = DisplayInfo { width_px: 1024, height_px: 768, hz: 60 };
        assert_eq!(wide.default_aspect_ratio(), "21:9");
        for (width_px, height_px) in [(5120, 1440), (3840, 1080), (7680, 2160)] {
            assert_eq!(
                DisplayInfo { width_px, height_px, hz: 60 }.default_aspect_ratio(),
                "32:9",
                "{width_px}x{height_px}"
            );
        }
        assert_eq!(DisplayInfo { width_px: 2560, height_px: 1080, hz: 60 }.default_aspect_ratio(), "21:9");
        assert_eq!(DisplayInfo { width_px: 1920, height_px: 1080, hz: 60 }.default_aspect_ratio(), "16:9");
        assert_eq!(laptop.default_aspect_ratio(), "16:10");
        assert_eq!(old.default_aspect_ratio(), "4:3");
    }

    #[test]
    fn scale_falls_back_to_two() {
        let scale = |text: &str| Settings { internal_resolution: text.into(), ..Settings::default() }.selected_scale();
        assert_eq!(scale("auto"), 2.0);
        assert_eq!(scale("10"), 2.0);
        assert_eq!(scale("9"), 2.0);
        assert_eq!(scale("8"), 8.0);
        assert_eq!(scale("6"), 6.0);
        assert_eq!(scale("3"), 3.0);
        assert_eq!(scale("0.5"), 0.5);
        assert_eq!(scale("0.1"), 0.25);
        assert_eq!(scale("0.75"), 0.75);
        assert_eq!(scale("x"), 2.0);
    }

    #[test]
    fn json_round_trip_uses_the_qml_names() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let settings = Settings { cheats_shortcut: "6+9".into(), ..Settings::default() };
        settings.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"cheatsShortcut\""));
        assert!(text.contains("\"progressive480p\""));
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn an_old_file_has_no_button_map_and_the_reset_clears_one() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(&path, r#"{"padButtons":"cross,circle,square,triangle","settingsVersion":11}"#).unwrap();
        let mut loaded = Settings::load(&path);
        assert_eq!(loaded.pad_map, "");
        loaded.pad_map = "a=cross;start=start".into();
        loaded.save(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"padMap\""));
        assert_eq!(Settings::load(&path).pad_map, "a=cross;start=start");
        loaded.pad_device = "0300dddd5e040000a102000000010000".into();
        loaded.pad_sdl_map = "a:b0,b:b1".into();
        loaded.reset_game_options(&DisplayInfo::default());
        assert_eq!((loaded.pad_map.as_str(), loaded.pad_device.as_str()), ("", ""));
        assert_eq!(loaded.pad_sdl_map, "a:b0,b:b1"); // the layout of a controller the system does not know survives a reset
    }

    #[test]
    fn auto_update_is_off_unless_the_file_says_otherwise() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        // A file written before the option existed.
        std::fs::write(&path, r#"{"gpu":"auto","settingsVersion":11}"#).unwrap();
        let mut loaded = Settings::load(&path);
        assert!(!loaded.auto_update);
        loaded.migrate(false, &DisplayInfo::default());
        assert!(!loaded.auto_update);
        std::fs::write(&path, r#"{"autoUpdate":true}"#).unwrap();
        assert!(Settings::load(&path).auto_update);
    }

    #[test]
    fn an_old_file_gets_a_neutral_image_adjustment_and_the_reset_restores_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(&path, r#"{"fxaa":false,"settingsVersion":12}"#).unwrap();
        let mut loaded = Settings::load(&path);
        loaded.migrate(false, &DisplayInfo::default());
        assert_eq!((loaded.brightness, loaded.contrast, loaded.gamma, loaded.saturation), (50, 50, 50, 50));
        loaded.brightness = 70;
        loaded.gamma = 20;
        loaded.save(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"brightness\": 70"));
        let mut reloaded = Settings::load(&path);
        assert_eq!((reloaded.brightness, reloaded.gamma), (70, 20));
        reloaded.reset_game_options(&DisplayInfo::default());
        assert_eq!((reloaded.brightness, reloaded.contrast, reloaded.gamma, reloaded.saturation), (50, 50, 50, 50));
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(&path, r#"{"gpu":"ahorro","futureField":1}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.gpu, "ahorro");
        assert_eq!(loaded.filter, "bilinear");
        assert_eq!(Settings::load(&directory.path().join("missing.json")), Settings::default());
    }
}
