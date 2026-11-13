//! Folders the launcher owns: organization `GOW_PC`, application `Launcher`. The first run imports the settings the old
//! Qt launcher left behind (see `legacy.rs`), so an upgrade keeps logs and shader cache in place.

use std::path::{Path, PathBuf};

pub const ORGANIZATION: &str = "GOW_PC";
pub const APPLICATION: &str = "Launcher";

#[derive(Debug, Clone)]
pub struct AppPaths {
    data: PathBuf,
    cache: PathBuf,
}

impl AppPaths {
    /// The per-user locations of the running system. `GOW_LAUNCHER_DATA_DIR` moves everything into one folder
    /// (portable use, and tests that must not touch the real data folder).
    pub fn system() -> Self {
        if let Some(folder) = std::env::var_os("GOW_LAUNCHER_DATA_DIR").filter(|v| !v.is_empty()) {
            let folder = PathBuf::from(folder);
            return Self { cache: folder.join("cache"), data: folder };
        }
        let data = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(ORGANIZATION).join(APPLICATION);
        let mut cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join(ORGANIZATION).join(APPLICATION);
        if cfg!(windows) {
            cache.push("cache");
        }
        Self { data, cache }
    }

    pub fn new(data: impl Into<PathBuf>, cache: impl Into<PathBuf>) -> Self {
        Self { data: data.into(), cache: cache.into() }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data.join("logs")
    }

    pub fn last_log(&self) -> PathBuf {
        self.logs_dir().join("last.log")
    }

    pub fn previous_log(&self) -> PathBuf {
        self.logs_dir().join("previous.log")
    }

    /// Where the game lists the joysticks it sees (`GOW_PAD_LIST_FILE`), for the Controls tab.
    pub fn pad_devices_file(&self) -> PathBuf {
        self.data.join("pads").join("devices.json")
    }

    /// The file whose freshness asks the game for the live raw input of the joysticks (see `controller::touch_probe`).
    pub fn pad_probe_file(&self) -> PathBuf {
        self.data.join("pads").join("probe")
    }

    /// Where the game writes that live raw input while the probe file is fresh.
    pub fn pad_live_file(&self) -> PathBuf {
        self.data.join("pads").join("live.json")
    }

    pub fn icon(&self) -> PathBuf {
        self.data.join("icon.png")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data.join("settings.json")
    }

    /// Written by the game while the player changes the cheats, overlay lines and image adjustments inside it (`ingame.rs`).
    pub fn ingame_state_file(&self) -> PathBuf {
        self.data.join("ingame-state.json")
    }

    pub fn shader_cache(&self) -> PathBuf {
        self.cache.join("shaders")
    }

    /// Where the extracted installation kit lives (see the installer).
    pub fn kit_dir(&self) -> PathBuf {
        self.data.join("kit")
    }
}

/// Folder the installer proposes and the launcher also probes when it does not find the game next to itself.
pub fn default_destination() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir).join("GodOfWar-PC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_live_under_the_data_dir() {
        let paths = AppPaths::new("/data", "/cache");
        assert_eq!(paths.last_log(), Path::new("/data/logs/last.log"));
        assert_eq!(paths.previous_log(), Path::new("/data/logs/previous.log"));
        assert_eq!(paths.shader_cache(), Path::new("/cache/shaders"));
    }
}
