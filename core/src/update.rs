//! Self-updater: checks a `latest.json` release manifest, downloads the new
//! binary, verifies its ed25519 signature, replaces the running executable and
//! relaunches. Replaces tauri-plugin-updater; pure Rust, no Tauri.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use std::collections::HashMap;

/// Public key (ed25519) matching the private key held as a CI secret.
pub const PUBKEY_B64: &str = "rOSGdeDvkEAq89Vp0yMOlAsj2DztU97z5JU0iyqjFok=";
pub const MANIFEST_URL: &str =
    "https://github.com/shadowlink/freeport/releases/latest/download/latest.json";

#[derive(Deserialize)]
struct PlatformEntry {
    url: String,
    signature: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    platforms: HashMap<String, PlatformEntry>,
}

#[derive(Clone)]
pub struct Update {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub signature: String,
}

fn is_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| s.split('.').filter_map(|x| x.parse::<u32>().ok()).collect::<Vec<_>>();
    parse(a) > parse(b)
}

pub fn platform_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// Returns an available update newer than `current`, or None.
pub async fn check(client: &reqwest::Client, current: &str) -> Option<Update> {
    let m: Manifest = client
        .get(MANIFEST_URL)
        .header("User-Agent", "freeport")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    if !is_newer(&m.version, current) {
        return None;
    }
    let p = m.platforms.get(platform_key())?;
    Some(Update { version: m.version, notes: m.notes, url: p.url.clone(), signature: p.signature.clone() })
}

/// Downloads, verifies (ed25519), replaces the current executable and relaunches.
/// On success it does not return (the process is replaced).
pub async fn apply(client: &reqwest::Client, upd: &Update) -> Result<(), String> {
    let bytes = client
        .get(&upd.url)
        .header("User-Agent", "freeport")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    let pk_bytes = B64.decode(PUBKEY_B64).map_err(|e| e.to_string())?;
    let pk = VerifyingKey::from_bytes(pk_bytes.as_slice().try_into().map_err(|_| "pubkey inválida")?)
        .map_err(|e| e.to_string())?;
    let sig_bytes = B64.decode(upd.signature.trim()).map_err(|e| e.to_string())?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| e.to_string())?;
    pk.verify(&bytes, &sig).map_err(|_| "firma inválida".to_string())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        // Windows locks a running executable, so it can't be overwritten. Rename
        // it aside (which IS allowed), write the new one in its place, relaunch.
        // The stale ".old" is cleaned up on the next start (see `cleanup`).
        let old = exe.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
        std::fs::write(&exe, &bytes).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let newp = exe.with_extension("new");
        std::fs::write(&newp, &bytes).map_err(|e| e.to_string())?;
        let _ = std::fs::set_permissions(&newp, std::fs::Permissions::from_mode(0o755));
        std::fs::rename(&newp, &exe).map_err(|e| e.to_string())?;
    }
    std::process::Command::new(&exe).spawn().map_err(|e| e.to_string())?;
    std::process::exit(0);
}

/// Removes a leftover ".old" executable left by a previous Windows self-update.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}
