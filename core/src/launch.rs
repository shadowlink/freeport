use crate::error::{AppError, AppResult};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Removes environment variables injected by the AppImage runtime so external
/// programs (the game, wine, umu-run, Proton) run in a clean system
/// environment. Launching an external binary with the AppImage's
/// `LD_LIBRARY_PATH`/`PYTHONHOME`/… is a classic cause of "nothing happens".
fn sanitize_env(cmd: &mut Command) {
    for var in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        "GTK_DATA_PREFIX",
        "GDK_PIXBUF_MODULE_FILE",
        "GDK_PIXBUF_MODULEDIR",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_PATH_1_0",
        "QT_PLUGIN_PATH",
        "PYTHONPATH",
        "PYTHONHOME",
        "PYTHONDONTWRITEBYTECODE",
        "PERLLIB",
        "GSETTINGS_SCHEMA_DIR",
        "GIO_MODULE_DIR",
        "GIO_EXTRA_MODULES",
        "GTK_IM_MODULE_FILE",
        "GTK_THEME",
        "LIBGL_DRIVERS_PATH",
        "GDK_BACKEND",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
    ] {
        cmd.env_remove(var);
    }
    // Strip the AppDir from PATH / XDG_DATA_DIRS so the child doesn't pick up
    // the AppImage's bundled tools/data.
    if let Ok(appdir) = std::env::var("APPDIR") {
        for var in ["PATH", "XDG_DATA_DIRS"] {
            if let Ok(val) = std::env::var(var) {
                let cleaned: Vec<&str> =
                    val.split(':').filter(|p| !p.starts_with(&appdir)).collect();
                cmd.env(var, cleaned.join(":"));
            }
        }
    }
    cmd.env_remove("APPDIR");
    cmd.env_remove("APPIMAGE");
    cmd.env_remove("ARGV0");
    cmd.env_remove("OWD");
}

/// Sends the child's stdout+stderr to `dir/dd-launch.log` so failures can be
/// inspected after the fact (the game runs detached).
fn log_to(dir: &Path) -> (Stdio, Stdio) {
    if let Ok(f) = std::fs::File::create(dir.join("dd-launch.log")) {
        if let Ok(f2) = f.try_clone() {
            return (Stdio::from(f), Stdio::from(f2));
        }
        return (Stdio::from(f), Stdio::null());
    }
    (Stdio::null(), Stdio::null())
}

/// Spawns the game binary detached, with its working directory set to the
/// install folder (most ports look for assets/ROM relative to the executable).
pub fn launch_binary(binary: &Path, envs: &std::collections::HashMap<String, String>) -> AppResult<Child> {
    if !binary.exists() {
        return Err(AppError::msg(format!(
            "el ejecutable no existe: {}",
            binary.display()
        )));
    }
    let cwd = binary
        .parent()
        .ok_or_else(|| AppError::msg("el ejecutable no tiene directorio padre"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Ensure the launch binary is executable (zip extraction may drop the bit).
        if let Ok(meta) = std::fs::metadata(binary) {
            let mut perms = meta.permissions();
            if perms.mode() & 0o111 == 0 {
                perms.set_mode(perms.mode() | 0o755);
                let _ = std::fs::set_permissions(binary, perms);
            }
        }
    }

    let (out, err) = log_to(cwd);
    let mut cmd = Command::new(binary);
    cmd.current_dir(cwd).stdout(out).stderr(err);
    sanitize_env(&mut cmd);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let child = cmd
        .spawn()
        .map_err(|e| AppError::msg(format!("no se pudo lanzar el juego: {e}")))?;
    Ok(child)
}

/// Launches a Windows `.exe` through the selected runner. `runner_id` is one of:
/// - "wine"            → system Wine
/// - "umu:"            → umu-run with auto Proton (GE-Proton)
/// - "umu:<path>"      → umu-run with a specific Proton at <path>
/// - "proton:<path>"   → raw Proton at <path>
/// `prefix` is a per-game Wine/Proton prefix so saves persist between runs.
pub fn launch_windows_with(exe: &Path, runner_id: &str, prefix: &Path, envs: &std::collections::HashMap<String, String>) -> AppResult<Child> {
    if !exe.exists() {
        return Err(AppError::msg(format!(
            "el ejecutable no existe: {}",
            exe.display()
        )));
    }
    let cwd = exe
        .parent()
        .ok_or_else(|| AppError::msg("el ejecutable no tiene directorio padre"))?;
    let _ = std::fs::create_dir_all(prefix);

    let mut cmd = if runner_id == "wine" {
        let mut c = Command::new("wine");
        c.arg(exe).env("WINEPREFIX", prefix);
        c
    } else if let Some(proton_path) = runner_id.strip_prefix("umu:") {
        let mut c = Command::new("umu-run");
        c.arg(exe).env("GAMEID", "0").env("WINEPREFIX", prefix);
        if !proton_path.is_empty() {
            c.env("PROTONPATH", proton_path);
        }
        c
    } else if let Some(proton_path) = runner_id.strip_prefix("proton:") {
        let steam_root = steam_root().unwrap_or_else(|| prefix.to_path_buf());
        let mut c = Command::new(format!("{proton_path}/proton"));
        c.args(["run"])
            .arg(exe)
            .env("STEAM_COMPAT_DATA_PATH", prefix)
            .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_root);
        c
    } else {
        return Err(AppError::msg(format!("runner desconocido: {runner_id}")));
    };

    let (out, err) = log_to(cwd);
    cmd.current_dir(cwd).stdout(out).stderr(err);
    sanitize_env(&mut cmd);
    let child = cmd.spawn().map_err(|e| {
        AppError::msg(format!(
            "no se pudo lanzar el build de Windows con «{runner_id}»: {e}. Comprueba que Wine/Proton (umu) está instalado."
        ))
    })?;
    Ok(child)
}

fn steam_root() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    for base in [".local/share/Steam", ".steam/steam", ".steam/root"] {
        let p = home.join(base);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}
