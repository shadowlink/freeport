//! Discord Rich Presence: shows the game the user is currently playing as their
//! Discord status ("Jugando <juego>"), with an elapsed timer.
//!
//! Talks to the local Discord IPC socket via the pure-Rust
//! `discord-rich-presence` crate — no native Discord SDK needed. Rich Presence
//! requires a Discord *Application ID* (created for free at
//! <https://discord.com/developers/applications>); it is public, not a secret.
//! The id comes from `Config.discord_app_id`, falling back to `DEFAULT_APP_ID`.

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Baked-in default Application ID. Empty = disabled unless the user sets one in
/// Ajustes. Fill this once you have registered the Freeport app on Discord.
pub const DEFAULT_APP_ID: &str = "1533785397329662052";

/// The Rich Presence art asset key uploaded in the Discord app portal
/// (Rich Presence → Art Assets). Upload the Freeport logo under this name.
const LARGE_IMAGE_KEY: &str = "freeport";

/// Resolves the effective Application ID: the user's config value takes
/// precedence, otherwise the baked-in default. Returns `None` when neither is
/// set (feature disabled).
pub fn resolve_app_id(cfg_value: Option<&str>) -> Option<String> {
    cfg_value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let d = DEFAULT_APP_ID.trim();
            (!d.is_empty()).then(|| d.to_string())
        })
}

/// Holds the (lazily connected) Discord IPC client. Connection is best-effort:
/// if Discord isn't running, calls are silently no-ops and retried next time.
pub struct DiscordPresence {
    /// `Some` once connected. Tracks the app_id it was opened with so we can
    /// reconnect if the user changes it.
    inner: Mutex<Option<(String, DiscordIpcClient)>>,
}

impl DiscordPresence {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Sets the presence to "playing `game`". `subtitle` is the second line
    /// (e.g. the port's name). No-op if `app_id` is empty or Discord is down.
    pub fn set_playing(&self, app_id: &str, game: &str, subtitle: &str) {
        if app_id.is_empty() {
            return;
        }
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };

        // (Re)connect if we have no client or the app_id changed.
        let need_new = match guard.as_ref() {
            Some((id, _)) => id != app_id,
            None => true,
        };
        if need_new {
            *guard = None;
            match DiscordIpcClient::new(app_id) {
                Ok(mut c) => {
                    if c.connect().is_ok() {
                        *guard = Some((app_id.to_string(), c));
                    } else {
                        return; // Discord not running / IPC unavailable.
                    }
                }
                Err(_) => return,
            }
        }

        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if let Some((_, client)) = guard.as_mut() {
            let activity = activity::Activity::new()
                .details(game)
                .state(subtitle)
                .assets(
                    activity::Assets::new()
                        .large_image(LARGE_IMAGE_KEY)
                        .large_text("Freeport"),
                )
                .timestamps(activity::Timestamps::new().start(start));
            // If the pipe broke, drop the client so we reconnect next time.
            if client.set_activity(activity).is_err() {
                *guard = None;
            }
        }
    }

    /// Clears the presence (called when the game exits).
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            if let Some((_, client)) = guard.as_mut() {
                if client.clear_activity().is_err() {
                    *guard = None;
                }
            }
        }
    }
}

impl Default for DiscordPresence {
    fn default() -> Self {
        Self::new()
    }
}
