//! What the player changes inside the game (the F2 cheat menu and the Port Options pages of the pause and title menus: cheats, the
//! performance overlay lines, the image adjustments, the frame rate: FPS limit, VSync, Early FINISH, and the image quality: render
//! resolution, upscaling filter, sharpness, FXAA) comes back to the launcher's settings, so the next session starts with it.
//!
//! The game process is given `GOW_INGAME_STATE=<data folder>/ingame-state.json`; whenever the player changes one of those switches
//! the game writes that file (a small JSON object with the current values, see `gow_persist.cpp`). The launcher reads it when the
//! game ends (and before the next start, in case the launcher itself was closed meanwhile), copies the values into the settings and
//! removes the file. Nothing else of the settings is touched.

use crate::settings::Settings;
use std::path::Path;

/// The ids of the cheats, in the order of the F2 menu: a public contract with the game (`PC_CHEATS`, `kIds` in gow_cheats.cpp).
pub const CHEAT_IDS: [&str; 9] = [
    "god_mode",
    "one_hit_kill",
    "infinite_magic",
    "infinite_rage",
    "max_orbs",
    "all_upgrades",
    "all_powers",
    "max_meters",
    "unlock_extras",
];

/// Copies the values of the game's state file (`text`) into `settings`. Returns whether anything was read. A value of the wrong
/// type is ignored, numbers are clamped to 0..=100, and cheat ids the game does not know are dropped. Under RetroAchievements
/// Hardcore the cheat list is never taken (the game does not send one then either).
pub fn apply(settings: &mut Settings, text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return false };
    let Some(object) = value.as_object() else { return false };
    let mut any = false;
    let mut flag = |key: &str, target: &mut bool| {
        if let Some(v) = object.get(key).and_then(|v| v.as_bool()) {
            *target = v;
            any = true;
        }
    };
    flag("showFps", &mut settings.show_fps);
    flag("showGpu", &mut settings.show_gpu);
    flag("showCpu", &mut settings.show_cpu);
    flag("showRam", &mut settings.show_ram);
    flag("vsync", &mut settings.vsync);
    flag("earlyFinish", &mut settings.early_finish);
    flag("fxaa", &mut settings.fxaa);
    let mut level = |key: &str, target: &mut i32| {
        if let Some(v) = object.get(key).and_then(|v| v.as_i64()) {
            *target = v.clamp(0, 100) as i32;
            any = true;
        }
    };
    level("brightness", &mut settings.brightness);
    level("contrast", &mut settings.contrast);
    level("gamma", &mut settings.gamma);
    level("saturation", &mut settings.saturation);
    if let Some(v) = object.get("internalResolution").and_then(|v| v.as_str()) {
        // the whole multiples the launcher offers (1x to 8x); anything else is ignored
        if matches!(v, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8") {
            settings.internal_resolution = v.to_string();
            any = true;
        }
    }
    if let Some(v) = object.get("filter").and_then(|v| v.as_str()) {
        if matches!(v, "fsr" | "bilinear" | "pixel") {
            settings.filter = v.to_string();
            any = true;
        }
    }
    if let Some(v) = object.get("sharpness").and_then(|v| v.as_f64()) {
        if v.is_finite() {
            settings.sharpness = (v.clamp(0.0, 1.0) * 100.0).round() / 100.0;
            any = true;
        }
    }
    if let Some(v) = object.get("fpsLimit").and_then(|v| v.as_i64()) {
        // the limits the launcher offers; anything else snaps to the nearest one like a stored value does
        settings.fps_limit = crate::launch::fps_limit(v.clamp(0, i64::from(i32::MAX)) as i32) as i32;
        any = true;
    }
    if let Some(list) = object.get("cheats").and_then(|v| v.as_str()) {
        if !(settings.retro_achievements_enabled && settings.retro_achievements_hardcore) {
            let wanted: Vec<&str> = list.split(',').map(str::trim).collect();
            let kept: Vec<&str> = CHEAT_IDS.iter().copied().filter(|id| wanted.contains(id)).collect();
            settings.cheats = kept.join(",");
            any = true;
        }
    }
    any
}

/// Reads and removes the state file; returns whether the settings changed from it.
pub fn consume(settings: &mut Settings, path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    let changed = apply(settings, &text);
    let _ = std::fs::remove_file(path);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_games_values_replace_the_settings() {
        let mut settings = Settings::default();
        let changed = apply(
            &mut settings,
            r#"{"version":1,"cheats":"god_mode,max_orbs","showFps":true,"showGpu":false,"showCpu":true,"showRam":true,
                "brightness":62,"contrast":40,"gamma":50,"saturation":100}"#,
        );
        assert!(changed);
        assert_eq!(settings.cheats, "god_mode,max_orbs");
        assert!(settings.show_fps && !settings.show_gpu && settings.show_cpu && settings.show_ram);
        assert_eq!((settings.brightness, settings.contrast, settings.gamma, settings.saturation), (62, 40, 50, 100));
    }

    #[test]
    fn bad_input_is_ignored_or_clamped_and_unknown_cheats_are_dropped() {
        let mut settings = Settings::default();
        assert!(!apply(&mut settings, "not json"));
        assert!(!apply(&mut settings, "[1,2]"));
        assert!(apply(
            &mut settings,
            r#"{"brightness":900,"contrast":-5,"gamma":"x","cheats":"max_orbs, nope,god_mode"}"#
        ));
        assert_eq!((settings.brightness, settings.contrast, settings.gamma), (100, 0, 50));
        assert_eq!(settings.cheats, "god_mode,max_orbs", "kept in the order of the menu, unknown ids dropped");
    }

    #[test]
    fn what_the_game_really_writes_is_understood() {
        // copied from a run of the game (gow_integration.cpp, `[gow-persist] wrote ...`), trailing newline included
        let mut settings = Settings::default();
        let text = "{\"version\":1,\"cheats\":\"god_mode\",\"showFps\":false,\"showGpu\":false,\"showCpu\":false,\"showRam\":false,\"brightness\":60,\"contrast\":48,\"gamma\":50,\"saturation\":50}
";
        assert!(apply(&mut settings, text));
        assert_eq!(settings.cheats, "god_mode");
        assert_eq!((settings.brightness, settings.contrast, settings.gamma, settings.saturation), (60, 48, 50, 50));
    }

    #[test]
    fn the_frame_rate_comes_back_too() {
        let mut settings = Settings::default();
        assert!(apply(&mut settings, r#"{"fpsLimit":120,"vsync":true,"earlyFinish":false}"#));
        assert_eq!(settings.fps_limit, 120);
        assert!(settings.vsync);
        assert!(!settings.early_finish);
        assert!(apply(&mut settings, r#"{"fpsLimit":200}"#));
        assert_eq!(settings.fps_limit, 120, "snapped to a limit the launcher offers, never above the cap");
        assert!(apply(&mut settings, r#"{"fpsLimit":-3}"#));
        assert_eq!(settings.fps_limit, 60);
        // without the worker the game sends no earlyFinish: the setting stays
        let mut kept = Settings { early_finish: true, ..Settings::default() };
        assert!(apply(&mut kept, r#"{"vsync":false}"#));
        assert!(kept.early_finish);
    }

    #[test]
    fn the_image_quality_comes_back_too() {
        let mut settings = Settings::default();
        assert!(apply(
            &mut settings,
            r#"{"internalResolution":"4","filter":"bilinear","sharpness":0.35,"fxaa":false,"progressive480p":false}"#
        ));
        assert_eq!(settings.internal_resolution, "4");
        assert_eq!(settings.filter, "bilinear");
        assert!((settings.sharpness - 0.35).abs() < 1e-9);
        // progressive scan is only set in the launcher: the game no longer sends it, and a file that has it is ignored
        assert!(!settings.fxaa && settings.progressive480p);
        // anything outside what the launcher offers is ignored
        assert!(!apply(&mut settings, r#"{"internalResolution":"9","filter":"lanczos"}"#));
        assert_eq!((settings.internal_resolution.as_str(), settings.filter.as_str()), ("4", "bilinear"));
        assert!(apply(&mut settings, r#"{"sharpness":7.5}"#));
        assert!((settings.sharpness - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_missing_cheat_key_leaves_the_list_alone() {
        let mut settings = Settings { cheats: "god_mode".into(), ..Settings::default() };
        assert!(apply(&mut settings, r#"{"showFps":true}"#));
        assert_eq!(settings.cheats, "god_mode");
    }

    #[test]
    fn the_file_is_consumed() {
        let dir = std::env::temp_dir().join(format!("gow-ingame-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ingame-state.json");
        std::fs::write(&path, r#"{"showRam":true}"#).unwrap();
        let mut settings = Settings::default();
        assert!(consume(&mut settings, &path));
        assert!(settings.show_ram);
        assert!(!path.exists());
        assert!(!consume(&mut settings, &path), "nothing to read the second time");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
