//! Starting the game without the launcher's window (issue #40): for Steam ("Add a non-Steam game") and shortcuts that
//! should open the game itself.
//!
//! The launcher still runs: it builds the game's environment from the settings, tracks the session, keeps the optimizer and
//! the play time, exactly as the PLAY button does. What it skips is the window, and with it WebView2. It closes when the
//! game closes, so Steam sees the session end. A start that cannot happen (the game is not installed, a launch error, a
//! game that closed with an error) opens the window instead, with the reason on screen.

use crate::game;
use crate::settings::Settings;

/// Starts the game directly, whatever the setting says (the "Play now" action of the shortcuts).
pub const PLAY_ARGUMENT: &str = "--play";
/// Opens the window, whatever the setting says.
pub const LAUNCHER_ARGUMENT: &str = "--launcher";
/// Opens the installation wizard (implies the window).
pub const WIZARD_ARGUMENT: &str = "--wizard";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    /// The launcher's window, as always.
    Window,
    /// The game, without the window.
    Direct,
}

/// What a start of the launcher with `arguments` does. `skip_launcher` is the player's setting ("Start the game
/// directly"); `launcher_key_held` is Shift held while the launcher opened (Windows), the way back to the window when the
/// setting is on and the launcher is started from Steam or a shortcut that cannot take arguments.
pub fn start_mode(arguments: &[String], skip_launcher: bool, launcher_key_held: bool) -> StartMode {
    let has = |wanted: &str| arguments.iter().skip(1).any(|argument| argument.eq_ignore_ascii_case(wanted));
    if has(LAUNCHER_ARGUMENT) || has(WIZARD_ARGUMENT) || launcher_key_held {
        StartMode::Window
    } else if has(PLAY_ARGUMENT) || skip_launcher {
        StartMode::Direct
    } else {
        StartMode::Window
    }
}

/// Why a direct start opens the window instead; `None` when the game can start. Checked before the start so the player
/// never waits on a launcher that is not on screen: the same checks the window makes before enabling PLAY.
pub fn not_ready(settings: &Settings) -> Option<&'static str> {
    if !game::check_game(&settings.elf, &settings.executable).valid {
        return Some("game");
    }
    if !game::check_data(&settings.elf, &settings.iso).ok {
        return Some("data");
    }
    if !game::check_executable(&settings.executable).executable {
        return Some("executable");
    }
    // Everything else (an interrupted "Optimize now", a launch error) is refused by the start itself, whose error the window
    // then shows.
    None
}

/// Whether Shift is held right now.
#[cfg(windows)]
pub fn launcher_key_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    // SAFETY: GetAsyncKeyState only reads the keyboard state; the high bit is set while the key is down.
    unsafe { GetAsyncKeyState(i32::from(VK_SHIFT)) as u16 & 0x8000 != 0 }
}

/// There is no portable way to read a key outside a window on Linux: `--launcher` is the way back there.
#[cfg(not(windows))]
pub fn launcher_key_held() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("gow-launcher").chain(list.iter().copied()).map(str::to_string).collect()
    }

    #[test]
    fn the_window_is_the_default() {
        assert_eq!(start_mode(&args(&[]), false, false), StartMode::Window);
    }

    #[test]
    fn the_setting_or_play_start_the_game() {
        assert_eq!(start_mode(&args(&[]), true, false), StartMode::Direct);
        assert_eq!(start_mode(&args(&["--play"]), false, false), StartMode::Direct);
        assert_eq!(start_mode(&args(&["--PLAY"]), false, false), StartMode::Direct);
    }

    #[test]
    fn shift_launcher_and_wizard_win_over_the_setting() {
        assert_eq!(start_mode(&args(&[]), true, true), StartMode::Window);
        assert_eq!(start_mode(&args(&["--play"]), true, true), StartMode::Window);
        assert_eq!(start_mode(&args(&["--launcher"]), true, false), StartMode::Window);
        assert_eq!(start_mode(&args(&["--play", "--launcher"]), false, false), StartMode::Window);
        assert_eq!(start_mode(&args(&["--wizard"]), true, false), StartMode::Window);
    }

    #[test]
    fn the_program_name_is_not_an_argument() {
        // A launcher renamed "--play" by someone does not start the game by itself.
        let arguments = vec!["--play".to_string()];
        assert_eq!(start_mode(&arguments, false, false), StartMode::Window);
    }

    #[test]
    fn a_missing_game_is_not_ready() {
        assert_eq!(not_ready(&Settings::default()), Some("game"));
    }
}
