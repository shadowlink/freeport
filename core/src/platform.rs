/// Returns the current platform "triple" used across the catalog for asset
/// matching, e.g. `linux-x86_64`, `windows-x86_64`, `macos-aarch64`.
///
/// It intentionally uses a coarse `os-arch` form (not the full Rust target
/// triple) because that is what the community projects encode in their release
/// asset names and what the catalog's `asset_rules`/`cached.platforms` use.
pub fn current_triple() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("{os}-{arch}")
}
