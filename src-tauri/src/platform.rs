/// Returns the current platform "triple" used across the catalog for asset
/// matching, e.g. `linux-x86_64`, `windows-x86_64`, `macos-aarch64`.
///
/// It intentionally uses a coarse `os-arch` form (not the full Rust target
/// triple) because that is what the community projects encode in their release
/// asset names and what the catalog's `asset_rules`/`cached.platforms` use.
pub fn current_triple() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        other => other, // "linux", "windows", "android", ...
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => other,
    };
    format!("{os}-{arch}")
}
