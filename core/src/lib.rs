//! freeport-core — backend logic reused by the native (Slint) app.
//! Extracted from the original Tauri app; contains no UI/Tauri dependencies.

pub mod actions;
pub mod discord;
pub mod error;
pub mod gamepad;
pub mod github;
pub mod groups;
pub mod install;
pub mod launch;
pub mod mods;
pub mod model;
pub mod pkg;
mod pkg_keys;
pub mod platform;
pub mod ra;
pub mod ra_mod;
pub mod steam;
pub mod store;
pub mod switch;
pub mod thumbs;
pub mod update;
pub mod wiki;
