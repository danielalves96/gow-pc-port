//! Launcher logic of the God of War (PS2) native port, without any UI. The Tauri app and the CLI are thin layers
//! over this crate, so everything here can be tested with `cargo test`.

pub mod activity;
pub mod controller;
pub mod credential;
pub mod direct;
pub mod display;
pub mod elf_check;
pub mod failure;
pub mod game;
pub mod gamepad;
pub mod gpu;
pub mod ingame;
pub mod installer;
pub mod iso;
pub mod iso_chd;
pub mod iso_patch;
pub mod keys;
pub mod kit;
pub mod launch;
pub mod legacy;
pub mod optimizer;
pub mod padmap;
pub mod paths;
pub mod process;
pub mod proctree;
pub mod rebuild;
pub mod report;
pub mod save_art;
pub mod saves;
pub mod script;
pub mod settings;
pub mod shortcut;
pub mod system;
pub mod textures;
pub mod update;
