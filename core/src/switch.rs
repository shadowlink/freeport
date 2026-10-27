//! Switch dumps for the Switch recompilations (LAS Recomp, B2recomp, Metroid
//! Dread…): they want a plain folder with `exefs/` and `romfs/`, not an
//! NSP/XCI/NSZ. Freeport ships no Nintendo keys and no decryption code: it only
//! drives the tools and keys the user already has (`nstool`, optionally `nsz`
//! for compressed dumps, and a `prod.keys` from their own console), and offers
//! the import only when all of them are present.

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::process::Command;

/// External tools and the user's key file, when all are available.
#[derive(Debug, Clone)]
pub struct Tools {
    pub nstool: PathBuf,
    /// Needed only for `.nsz` / `.xcz` inputs.
    pub nsz: Option<PathBuf>,
    pub keys: PathBuf,
}

fn find_bin(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join(".local/bin"));
    }
    for d in dirs {
        for n in [name.to_string(), format!("{name}.exe")] {
            let p = d.join(&n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Known places where Switch emulators and tools keep `prod.keys`.
pub fn key_candidates() -> Vec<PathBuf> {
    let h = dirs::home_dir().unwrap_or_default();
    let cfg = dirs::config_dir().unwrap_or_else(|| h.join(".config"));
    let data = dirs::data_dir().unwrap_or_else(|| h.join(".local/share"));
    vec![
        h.join(".switch/prod.keys"),
        cfg.join("Ryujinx/system/prod.keys"),
        h.join(".var/app/io.github.ryubing.Ryujinx/config/Ryujinx/system/prod.keys"),
        h.join(".var/app/org.ryujinx.Ryujinx/config/Ryujinx/system/prod.keys"),
        data.join("eden/keys/prod.keys"),
        h.join(".var/app/dev.eden_emu.eden/data/eden/keys/prod.keys"),
        data.join("yuzu/keys/prod.keys"),
        h.join(".var/app/org.yuzu_emu.yuzu/data/yuzu/keys/prod.keys"),
        data.join("suyu/keys/prod.keys"),
        h.join("AppData/Roaming/Ryujinx/system/prod.keys"),
        h.join("AppData/Roaming/yuzu/keys/prod.keys"),
    ]
}

/// Finds the tools and keys; the error says what is missing and where to put it.
pub fn detect() -> Result<Tools, String> {
    let nstool = find_bin("nstool");
    let keys = key_candidates().into_iter().find(|p| p.is_file());
    let mut missing = Vec::new();
    if nstool.is_none() {
        missing.push("nstool (en el PATH o en ~/.local/bin)".to_string());
    }
    if keys.is_none() {
        missing.push("prod.keys de tu consola (en ~/.switch/ o en la carpeta de claves de Ryujinx/Eden/yuzu)".to_string());
    }
    if !missing.is_empty() {
        return Err(format!("falta: {}", missing.join(" y ")));
    }
    Ok(Tools { nstool: nstool.unwrap(), nsz: find_bin("nsz"), keys: keys.unwrap() })
}

fn run(cmd: &mut Command, what: &str) -> AppResult<String> {
    let out = cmd.output().map_err(|e| AppError::msg(format!("no se pudo ejecutar {what}: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if stdout.contains("not readable") || String::from_utf8_lossy(&out.stderr).contains("not readable") {
        return Err(AppError::msg(format!(
            "{what}: nstool no pudo descifrar una partición (claves o ticket insuficientes)"
        )));
    }
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.lines().chain(stdout.lines()).rfind(|l| !l.is_empty()).unwrap_or("").chars().take(300).collect();
        return Err(AppError::msg(format!("{what} falló: {tail}")));
    }
    Ok(stdout)
}

/// Copy of the user's key file with only well-formed `name = hex` lines:
/// emulator exports often carry entries hactool rejects outright (it then
/// refuses the whole file). Written 0600 inside the import's temp folder.
fn filtered_keyset(src: &Path, dest: &Path) -> AppResult<PathBuf> {
    let text = std::fs::read_to_string(src)?;
    let mut out = String::new();
    for line in text.lines() {
        let Some((name, val)) = line.split_once('=') else { continue };
        let (name, val) = (name.trim(), val.trim());
        let ok_name = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let ok_val = !val.is_empty() && val.len() % 2 == 0 && val.chars().all(|c| c.is_ascii_hexdigit())
            && matches!(val.len(), 32 | 64 | 96 | 128 | 256 | 512 | 1056);
        if ok_name && ok_val {
            out.push_str(name);
            out.push_str(" = ");
            out.push_str(val);
            out.push('\n');
        }
    }
    std::fs::write(dest, out)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o600));
    }
    Ok(dest.to_path_buf())
}

/// Rights id and (still encrypted) title key of a `.tik`, as the tools expect
/// them on the command line. Never logged.
fn ticket_key(tik: &Path) -> AppResult<(String, String)> {
    let b = std::fs::read(tik)?;
    let sig_type = u32::from_le_bytes(b.get(0..4).ok_or_else(|| AppError::msg("ticket vacío"))?.try_into().unwrap());
    // Body offset = 4 + signature + padding (RSA-4096 0x240, RSA-2048 0x140, ECDSA 0x80).
    let body = match sig_type {
        0x10000 | 0x10003 => 0x240,
        0x10001 | 0x10004 => 0x140,
        0x10002 | 0x10005 => 0x80,
        _ => return Err(AppError::msg("ticket con firma desconocida")),
    };
    let key = b.get(body + 0x40..body + 0x50).ok_or_else(|| AppError::msg("ticket truncado"))?;
    let rid = b.get(body + 0x160..body + 0x170).ok_or_else(|| AppError::msg("ticket truncado"))?;
    let hex = |x: &[u8]| x.iter().map(|c| format!("{c:02x}")).collect::<String>();
    Ok((hex(rid), hex(key)))
}

#[derive(Debug, Clone)]
struct Content {
    /// "Application" | "Patch" | "AddOnContent" | other
    kind: String,
    /// Base application id (hex, 16 chars, lowercase).
    app_id: String,
    version: u64,
    program_nca: PathBuf,
    tik: Option<PathBuf>,
}

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| {
        let t = l.trim_start();
        t.strip_prefix(key).map(|v| v.trim_start_matches(':').trim())
    })
}

/// Reads the content meta of an unpacked NSP/XCI partition directory.
fn describe(tools: &Tools, home: &Path, pfs: &Path, scratch: &Path) -> AppResult<Content> {
    let entries: Vec<PathBuf> = std::fs::read_dir(pfs)?.flatten().map(|e| e.path()).collect();
    let cnmt_nca = entries
        .iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(".cnmt.nca")).unwrap_or(false))
        .ok_or_else(|| AppError::msg("el paquete no tiene metadatos (.cnmt.nca); ¿es un NSP de juego?"))?;
    let cdir = scratch.join("cnmt");
    std::fs::create_dir_all(&cdir)?;
    run(nstool(tools, home).arg("-x").arg("/0").arg(&cdir).arg(cnmt_nca), "nstool (cnmt)")?;
    let cnmt = std::fs::read_dir(&cdir)?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("cnmt"))
        .ok_or_else(|| AppError::msg("no se pudo leer el .cnmt"))?;
    let meta = run(nstool(tools, home).arg(&cnmt), "nstool (meta)")?;
    let kind = field(&meta, "Type").unwrap_or("").split(' ').next().unwrap_or("").to_string();
    let title = field(&meta, "TitleId").unwrap_or("").trim_start_matches("0x").to_lowercase();
    let app_id = field(&meta, "ApplicationId")
        .map(|v| v.trim_start_matches("0x").to_lowercase())
        .unwrap_or_else(|| title.clone());
    let version = field(&meta, "Version")
        .and_then(|v| v.rsplit("(v").next())
        .and_then(|v| v.trim_end_matches(')').parse::<u64>().ok())
        .unwrap_or(0);
    // ContentInfo: the entry whose Type is Program gives the NCA id.
    let mut program_id = None;
    let mut lines = meta.lines().map(|l| l.trim()).peekable();
    while let Some(l) = lines.next() {
        if l.starts_with("Type:") && l.contains("Program") {
            for n in lines.by_ref() {
                if let Some(id) = n.strip_prefix("Id:") {
                    program_id = Some(id.trim().to_string());
                    break;
                }
            }
            break;
        }
    }
    let program_id = program_id.ok_or_else(|| AppError::msg("el paquete no contiene el programa del juego"))?;
    let program_nca = pfs.join(format!("{program_id}.nca"));
    if !program_nca.is_file() {
        return Err(AppError::msg(format!("falta el NCA del programa ({program_id}.nca) en el paquete")));
    }
    let tik = entries.iter().find(|p| p.extension().and_then(|e| e.to_str()) == Some("tik")).cloned();
    Ok(Content { kind, app_id, version, program_nca, tik })
}

/// Imports NSP/XCI/NSZ/XCZ dumps (base game, then updates) into
/// `<install_dir>/<APPID>/{exefs,romfs}` and returns that folder.
pub fn import(
    tools: &Tools,
    install_dir: &Path,
    import_dir: Option<&str>,
    expect: (Option<&str>, Option<&str>),
    files: &[PathBuf],
    progress: &mut dyn FnMut(&str),
) -> AppResult<PathBuf> {
    let tmp = install_dir.join(".switch-import");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    // The tools run with a private HOME inside the temp folder: a cleaned copy
    // of prod.keys plus a title.keys built from the packages' own tickets, so
    // nstool can open an update against its base (two title keys at once).
    // The user's real key files are never touched.
    let home = tmp.join("home");
    let switch_dir = home.join(".switch");
    let result = match std::fs::create_dir_all(&switch_dir)
        .map_err(AppError::from)
        .and_then(|_| filtered_keyset(&tools.keys, &switch_dir.join("prod.keys")))
    {
        Ok(keys) => {
            let tools = Tools { keys, ..tools.clone() };
            import_inner(&tools, install_dir, import_dir, expect, &tmp, &home, files, progress)
        }
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn nstool(tools: &Tools, home: &Path) -> Command {
    let mut c = Command::new(&tools.nstool);
    c.env("HOME", home).env("USERPROFILE", home).arg("-k").arg(&tools.keys);
    c
}

fn import_inner(
    tools: &Tools,
    install_dir: &Path,
    import_dir: Option<&str>,
    (expect_build_id, expect_version): (Option<&str>, Option<&str>),
    tmp: &Path,
    home: &Path,
    files: &[PathBuf],
    progress: &mut dyn FnMut(&str),
) -> AppResult<PathBuf> {
    let mut contents: Vec<Content> = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("archivo").to_string();
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let mut src = f.clone();
        if ext == "nsz" || ext == "xcz" {
            let nsz = tools.nsz.as_ref().ok_or_else(|| {
                AppError::msg("este archivo está comprimido (.nsz/.xcz) y hace falta la herramienta nsz (pip install nsz)")
            })?;
            progress(&format!("Descomprimiendo {name}"));
            let out = tmp.join(format!("nsz{i}"));
            std::fs::create_dir_all(&out)?;
            run(Command::new(nsz).arg("-D").arg("--keys").arg(&tools.keys).arg("-o").arg(&out).arg(f), "nsz")?;
            src = std::fs::read_dir(&out)?
                .flatten()
                .map(|e| e.path())
                .find(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("nsp") | Some("xci")))
                .ok_or_else(|| AppError::msg("nsz no produjo ningún NSP/XCI"))?;
        }
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        progress(&format!("Abriendo {name}"));
        let pfs = tmp.join(format!("pfs{i}"));
        std::fs::create_dir_all(&pfs)?;
        let mut c = nstool(tools, home);
        if ext == "xci" {
            c.arg("-x").arg("/secure").arg(&pfs).arg(&src);
        } else {
            c.arg("-x").arg(&pfs).arg(&src);
        }
        run(&mut c, "nstool")?;
        let scratch = tmp.join(format!("meta{i}"));
        std::fs::create_dir_all(&scratch)?;
        contents.push(describe(tools, home, &pfs, &scratch)?);
        // Register this package's ticket for nstool (title.keys: rights id = key).
        if let Some(tik) = contents.last().and_then(|c| c.tik.clone()) {
            let (rid, key) = ticket_key(&tik)?;
            let tk = home.join(".switch/title.keys");
            let mut text = std::fs::read_to_string(&tk).unwrap_or_default();
            text.push_str(&format!("{rid} = {key}\n"));
            std::fs::write(&tk, text)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tk, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    let base = contents.iter().find(|c| c.kind == "Application").cloned();
    let app_id = base
        .as_ref()
        .map(|b| b.app_id.clone())
        .or_else(|| contents.first().map(|c| c.app_id.clone()))
        .unwrap_or_default();
    if contents.iter().any(|c| c.app_id != app_id) {
        return Err(AppError::msg("los archivos elegidos son de juegos distintos"));
    }
    let base = base.ok_or_else(|| {
        AppError::msg("hace falta el juego base (NSP/XCI de la aplicación) en la misma importación; las actualizaciones solas no bastan")
    })?;
    let dest = match import_dir {
        Some(rel) => install_dir.join(rel.replace("{title_id}", &app_id.to_uppercase())),
        None => install_dir.join(app_id.to_uppercase()),
    };
    let exefs = dest.join("exefs");
    let romfs = dest.join("romfs");
    let _ = std::fs::remove_dir_all(&exefs);
    let _ = std::fs::remove_dir_all(&romfs);
    std::fs::create_dir_all(&dest)?;

    let extract = |nca: &Path, basenca: Option<&PathBuf>, part: &str, out: &Path| -> AppResult<()> {
        let mut c = nstool(tools, home);
        if let Some(b) = basenca {
            c.arg("--basenca").arg(b);
        }
        c.arg("-x").arg(part).arg(out).arg(nca);
        run(&mut c, "nstool (extracción)").map(|_| ())
    };
    let mut patches: Vec<&Content> = contents.iter().filter(|c| c.kind == "Patch").collect();
    patches.sort_by_key(|c| c.version);
    match patches.last() {
        // An update NCA read against the base NCA yields the complete, merged
        // exefs and romfs of that version, so only the newest update is needed.
        Some(p) => {
            let v = p.version >> 16;
            progress(&format!("Extrayendo exefs (actualización v{v})"));
            extract(&p.program_nca, Some(&base.program_nca), "/0", &exefs)?;
            progress(&format!("Extrayendo romfs (actualización v{v}, puede tardar)"));
            extract(&p.program_nca, Some(&base.program_nca), "/1", &romfs)?;
        }
        None => {
            progress("Extrayendo exefs del juego base");
            extract(&base.program_nca, None, "/0", &exefs)?;
            progress("Extrayendo romfs del juego base (puede tardar)");
            extract(&base.program_nca, None, "/1", &romfs)?;
        }
    }
    let romfs_files = std::fs::read_dir(&romfs).map(|d| d.count()).unwrap_or(0);
    if romfs_files == 0 {
        return Err(AppError::msg("la extracción terminó sin romfs; revisa las claves (prod.keys) y el archivo"));
    }
    if !exefs.join("main").is_file() {
        return Err(AppError::msg("la extracción terminó pero no hay exefs/main; revisa las claves (prod.keys) y el archivo"));
    }
    if let Some(want) = expect_build_id {
        let got = nso_build_id(&exefs.join("main"));
        if !got.starts_with(&want.to_uppercase()) {
            let ver = expect_version.unwrap_or("la versión exacta que pide el port");
            let hint = if patches.is_empty() {
                "solo se ha importado el juego base: añade a la misma importación el NSP/NSZ de la actualización"
            } else {
                "la actualización importada no es esa versión: usa el NSP/NSZ de la actualización correcta"
            };
            return Err(AppError::msg(format!(
                "el volcado importado no es {ver} (build id de exefs/main {got}, esperado {want}…): {hint}"
            )));
        }
    }
    Ok(dest)
}

/// Build id (hex, uppercase) in an NSO header; empty when unreadable.
pub fn nso_build_id(main: &Path) -> String {
    use std::io::Read;
    let mut buf = [0u8; 0x60];
    match std::fs::File::open(main).and_then(|mut f| f.read_exact(&mut buf)) {
        Ok(()) if &buf[0..4] == b"NSO0" => buf[0x40..0x60].iter().map(|b| format!("{b:02X}")).collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nso_build_id_reads_header_and_rejects_other_files() {
        let tmp = std::env::temp_dir().join(format!("fp-nso-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let mut h = vec![0u8; 0x60];
        h[0..4].copy_from_slice(b"NSO0");
        h[0x40..0x48].copy_from_slice(&[0xFB, 0x08, 0xF1, 0xD2, 0x0F, 0xD1, 0x20, 0x4F]);
        std::fs::write(tmp.join("main"), &h).unwrap();
        assert!(nso_build_id(&tmp.join("main")).starts_with("FB08F1D20FD1204F"));
        std::fs::write(tmp.join("other"), b"not an nso at all, far too short").unwrap();
        assert_eq!(nso_build_id(&tmp.join("other")), "");
        assert_eq!(nso_build_id(&tmp.join("missing")), "");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parses_meta_fields() {
        let meta = "[ContentMeta]\n  TitleId:               0x01006bb00c6f0800\n  Version:               0.0.2-0.0 (v131072)\n  Type:                  Patch (129)\n  PatchMetaExtendedHeader:\n    ApplicationId:         0x01006bb00c6f0000\n  ContentInfo:\n    0\n      Type:         Program (1)\n      Id:           88d7f50b596192d406c20e6c395dfb80\n";
        assert_eq!(field(meta, "Type").unwrap().split(' ').next(), Some("Patch"));
        assert_eq!(field(meta, "ApplicationId"), Some("0x01006bb00c6f0000"));
        let v = field(meta, "Version").and_then(|v| v.rsplit("(v").next()).and_then(|v| v.trim_end_matches(')').parse::<u64>().ok());
        assert_eq!(v, Some(131072));
    }
}
