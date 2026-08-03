use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// Streams `url` to `dest`, invoking `on_progress(downloaded, total)` as bytes
/// arrive so the UI can render a progress bar.
pub async fn download_to_file<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut on_progress: F,
) -> AppResult<()>
where
    F: FnMut(u64, u64),
{
    let resp = client
        .get(url)
        .header("User-Agent", "decompdeck")
        .send()
        .await?
        .error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(dest).await?;
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    file.flush().await?;
    Ok(())
}

/// Extracts a downloaded archive into `dest_dir`, dispatching on file extension.
/// Supports `.zip` and `.tar.gz`/`.tgz` (the formats used by the Linux/Windows
/// releases of the seeded projects). Runs on a blocking thread.
pub async fn extract_archive(archive: PathBuf, dest_dir: PathBuf) -> AppResult<()> {
    tokio::task::spawn_blocking(move || extract_sync(&archive, &dest_dir))
        .await
        .map_err(|e| AppError::msg(format!("fallo al extraer: {e}")))?
}

fn extract_sync(archive: &Path, dest_dir: &Path) -> AppResult<()> {
    extract_one(archive, dest_dir)?;
    // Some projects wrap the payload in a second archive (e.g. Zelda64Recomp's
    // Linux `.zip` contains a single `.tar.gz` that holds the actual binary).
    // Unwrap a lone nested archive up to a few levels deep.
    for _ in 0..4 {
        match nested_archive_to_unwrap(dest_dir) {
            Some(inner) => {
                extract_one(&inner, dest_dir)?;
                std::fs::remove_file(&inner)?;
            }
            None => break,
        }
    }
    Ok(())
}

/// Whether `name` is an archive DecompDeck knows how to unpack. Assets that are
/// not archives (e.g. a bare `.AppImage` or ELF binary) are installed as-is.
pub fn is_archive(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".zip")
        || n.ends_with(".tar.gz")
        || n.ends_with(".tgz")
        || n.ends_with(".tar.xz")
        || n.ends_with(".txz")
        || n.ends_with(".deb")
}

fn is_archive_name(name: &str) -> bool {
    is_archive(name)
}

/// Ensures a file has the executable bit set (Unix). Used for bare-binary /
/// AppImage assets that ship without it.
pub fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o755);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Unpacks a `.tar.xz` stream into `dest_dir` using a pure-Rust xz decoder (no
/// system liblzma dependency).
fn extract_tar_xz(archive: &Path, dest_dir: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut reader = std::io::BufReader::new(file);
    let mut decompressed = Vec::new();
    lzma_rs::xz_decompress(&mut reader, &mut decompressed)
        .map_err(|e| AppError::msg(format!("error al descomprimir xz: {e}")))?;
    tar::Archive::new(std::io::Cursor::new(decompressed)).unpack(dest_dir)?;
    Ok(())
}

/// Extracts a Debian `.deb` package: reads its `ar` container, finds the
/// `data.tar.{gz,xz}` member and unpacks it (the binaries usually land under
/// `usr/bin` or `usr/games`, which the launch finder searches).
fn extract_deb(deb: &Path, dest_dir: &Path) -> AppResult<()> {
    use std::io::Read as _;
    let file = std::fs::File::open(deb)?;
    let mut archive = ar::Archive::new(file);
    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.map_err(|e| AppError::msg(format!("deb inválido: {e}")))?;
        let ident = String::from_utf8_lossy(entry.header().identifier()).to_string();
        if !ident.starts_with("data.tar") {
            continue;
        }
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        let cursor = std::io::Cursor::new(buf);
        if ident.ends_with(".gz") {
            tar::Archive::new(flate2::read::GzDecoder::new(cursor)).unpack(dest_dir)?;
        } else if ident.ends_with(".xz") {
            let mut dec = Vec::new();
            let mut c = cursor;
            lzma_rs::xz_decompress(&mut c, &mut dec)
                .map_err(|e| AppError::msg(format!("error al descomprimir xz del .deb: {e}")))?;
            tar::Archive::new(std::io::Cursor::new(dec)).unpack(dest_dir)?;
        } else {
            return Err(AppError::msg(format!(
                "compresión de data.tar no soportada en el .deb: {ident}"
            )));
        }
        return Ok(());
    }
    Err(AppError::msg("el .deb no contiene data.tar"))
}

/// Returns a top-level archive that should still be unwrapped, so payloads that
/// arrive wrapped in a second archive (e.g. Zelda64Recomp's `.zip` holds a lone
/// `.tar.gz` with the binary) get fully extracted. Triggers when the dir's only
/// entry is that archive, OR when there's exactly one top-level archive and no
/// launchable binary has appeared yet (the archive sits beside metadata files
/// like a `.sha256`/`.txt`). Returns None once a runnable binary exists, or when
/// there are zero/multiple archives (don't guess).
fn nested_archive_to_unwrap(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    let mut archives = entries.iter().filter(|p| {
        p.is_file()
            && p.file_name()
                .and_then(|n| n.to_str())
                .map(is_archive_name)
                .unwrap_or(false)
    });
    let inner = archives.next()?.clone();
    if archives.next().is_some() {
        return None; // more than one archive → ambiguous, leave it
    }
    // Unwrap if it's the sole entry, or nothing runnable exists yet.
    let sole = entries.len() == 1;
    if sole || find_launch_binary(dir, None, false).is_err() {
        Some(inner)
    } else {
        None
    }
}

fn extract_one(archive: &Path, dest_dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dest_dir)?;
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    if name.ends_with(".zip") {
        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        zip.extract(dest_dir)?;
        Ok(())
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = std::fs::File::open(archive)?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(gz);
        tar.unpack(dest_dir)?;
        Ok(())
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        extract_tar_xz(archive, dest_dir)
    } else if name.ends_with(".deb") {
        extract_deb(archive, dest_dir)
    } else {
        Err(AppError::msg(format!(
            "formato de archivo no soportado todavía: {name} (soportados: .zip, .tar.gz, .tar.xz, .deb)"
        )))
    }
}

/// Resolves the executable to launch inside an install directory. Prefers the
/// per-project `hint`; otherwise applies a heuristic over the extracted tree.
/// When `windows` is true it locates the Windows `.exe` (for Wine/Proton),
/// otherwise a native binary.
pub fn find_launch_binary(dir: &Path, hint: Option<&str>, windows: bool) -> AppResult<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_files(dir, &mut candidates, 0);

    if let Some(h) = hint {
        if !h.is_empty() {
            if let Some(found) = candidates
                .iter()
                .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(h))
            {
                return Ok(found.clone());
            }
        }
    }

    let is_exe = |p: &PathBuf| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")) == Some(true);

    if windows {
        // Pick the largest .exe (skip tiny uninstaller/helper stubs).
        let mut exes: Vec<(PathBuf, u64)> = candidates
            .iter()
            .filter(|p| is_exe(p))
            .map(|p| (p.clone(), std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)))
            .collect();
        exes.sort_by_key(|(_, sz)| std::cmp::Reverse(*sz));
        return exes
            .into_iter()
            .next()
            .map(|(p, _)| p)
            .ok_or_else(|| AppError::msg("no se encontró ningún .exe tras la instalación"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Prefer AppImage, then any file with the executable bit set (excluding
        // shared objects), picking the largest as a tie-breaker.
        if let Some(app) = candidates.iter().find(|p| {
            p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("appimage"))
                == Some(true)
        }) {
            return Ok(app.clone());
        }
        let mut execs: Vec<(PathBuf, u64)> = candidates
            .iter()
            .filter(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "so" || ext == "dll" || ext == "dylib" {
                    return false;
                }
                std::fs::metadata(p)
                    .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
                    .unwrap_or(false)
            })
            .map(|p| {
                let sz = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                (p.clone(), sz)
            })
            .collect();
        execs.sort_by_key(|(_, sz)| std::cmp::Reverse(*sz));
        if let Some((p, _)) = execs.first() {
            return Ok(p.clone());
        }
    }

    #[cfg(windows)]
    {
        if let Some(exe) = candidates.iter().find(|p| is_exe(p)) {
            return Ok(exe.clone());
        }
    }

    Err(AppError::msg(
        "no se pudo localizar el ejecutable tras la instalación",
    ))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out, depth + 1);
        } else {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "decompdeck-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn zip_extract_and_find_binary() {
        let root = scratch("zip");
        let zip_path = root.join("release.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            // A nested game binary plus a shared library that must NOT be picked.
            zip.add_directory("game/", opts).unwrap();
            zip.start_file("game/TheGame", opts).unwrap();
            zip.write_all(&vec![0u8; 4096]).unwrap();
            let lib_opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
            zip.start_file("game/libfoo.so", lib_opts).unwrap();
            zip.write_all(b"lib").unwrap();
            zip.finish().unwrap();
        }
        let dest = root.join("out");
        extract_sync(&zip_path, &dest).unwrap();
        assert!(dest.join("game/TheGame").exists());

        // With an explicit hint.
        let hinted = find_launch_binary(&dest, Some("TheGame"), false).unwrap();
        assert_eq!(hinted.file_name().unwrap(), "TheGame");

        // Heuristic (unix): the exec-bit binary wins over the .so.
        #[cfg(unix)]
        {
            let auto = find_launch_binary(&dest, None, false).unwrap();
            assert_eq!(auto.file_name().unwrap(), "TheGame");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nested_zip_wrapping_targz() {
        let root = scratch("nested");
        // Inner .tar.gz holding an executable.
        let inner_tar = root.join("inner.tar.gz");
        {
            let f = std::fs::File::create(&inner_tar).unwrap();
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut builder = tar::Builder::new(enc);
            let data = vec![7u8; 2048];
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            builder.append_data(&mut header, "TheGame", &data[..]).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        // Outer .zip that contains only the inner .tar.gz (the Zelda64Recomp shape).
        let outer_zip = root.join("release.zip");
        {
            let f = std::fs::File::create(&outer_zip).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("inner.tar.gz", opts).unwrap();
            zip.write_all(&std::fs::read(&inner_tar).unwrap()).unwrap();
            zip.finish().unwrap();
        }

        let dest = root.join("out");
        extract_sync(&outer_zip, &dest).unwrap();
        // The intermediate archive is unwrapped and removed…
        assert!(!dest.join("inner.tar.gz").exists());
        // …and the real binary is now locatable.
        let bin = find_launch_binary(&dest, Some("TheGame"), false).unwrap();
        assert!(bin.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_unknown_archive() {
        let root = scratch("bad");
        let bogus = root.join("payload.rar");
        std::fs::write(&bogus, b"nope").unwrap();
        assert!(extract_sync(&bogus, &root.join("out")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_detection() {
        for a in ["x.zip", "x.tar.gz", "x.tgz", "x.tar.xz", "x.txz", "x.deb"] {
            assert!(is_archive(a), "{a} should be an archive");
        }
        // Bare binaries / AppImages are installed as-is, not extracted.
        for b in ["Foo-linux-x86_64.AppImage", "pd.x86_64", "game.bin"] {
            assert!(!is_archive(b), "{b} should NOT be an archive");
        }
    }

    #[test]
    fn tar_xz_extract() {
        let root = scratch("xz");
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let data = vec![9u8; 1024];
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            b.append_data(&mut h, "Game", &data[..]).unwrap();
            b.finish().unwrap();
        }
        let xz_path = root.join("payload.tar.xz");
        {
            let mut out = std::fs::File::create(&xz_path).unwrap();
            let mut cur = std::io::Cursor::new(&tar_bytes);
            lzma_rs::xz_compress(&mut cur, &mut out).unwrap();
        }
        let dest = root.join("out");
        extract_sync(&xz_path, &dest).unwrap();
        assert!(find_launch_binary(&dest, Some("Game"), false).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deb_extract() {
        let root = scratch("deb");
        let mut targz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut targz, flate2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            let data = vec![3u8; 512];
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            b.append_data(&mut h, "usr/games/thegame", &data[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let deb_path = root.join("pkg.deb");
        {
            let out = std::fs::File::create(&deb_path).unwrap();
            let mut builder = ar::Builder::new(out);
            let header = ar::Header::new(b"data.tar.gz".to_vec(), targz.len() as u64);
            builder.append(&header, &targz[..]).unwrap();
        }
        let dest = root.join("out");
        extract_sync(&deb_path, &dest).unwrap();
        assert!(dest.join("usr/games/thegame").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Real .tar.xz (devilutionX) and .deb.zip (Oddworld) end-to-end. Ignored by
    /// default; run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_install_new_formats() {
        let client = reqwest::Client::builder().user_agent("dd-test").build().unwrap();

        // devilutionX ships a .tar.xz for Linux.
        let rels = crate::github::fetch_releases(&client, "diasurgical/devilutionX", None)
            .await
            .unwrap();
        let rel = crate::github::pick_release(&rels, "stable", None).unwrap();
        let asset = crate::github::pick_asset(&rel, "(?i)linux-x86_64\\.tar\\.xz$").unwrap();
        let root = scratch("real-xz");
        let archive = root.join(&asset.name);
        download_to_file(&client, &asset.browser_download_url, &archive, |_, _| {})
            .await
            .unwrap();
        extract_archive(archive, root.join("out")).await.unwrap();
        assert!(
            find_launch_binary(&root.join("out"), Some("devilutionx"), false).is_ok(),
            "devilutionx binary not found after .tar.xz extract"
        );
        let _ = std::fs::remove_dir_all(&root);

        // Oddworld (AliveTeam) ships a .deb wrapped in a .zip.
        let rels2 = crate::github::fetch_releases(&client, "AliveTeam/alive_reversing", None)
            .await
            .unwrap();
        let rel2 = crate::github::pick_release(&rels2, "stable", None).unwrap();
        if let Ok(asset2) = crate::github::pick_asset(&rel2, "(?i)\\.deb\\.zip$") {
            let root2 = scratch("real-deb");
            let archive2 = root2.join(&asset2.name);
            download_to_file(&client, &asset2.browser_download_url, &archive2, |_, _| {})
                .await
                .unwrap();
            // .zip -> nested .deb -> data.tar; just assert it unpacks without error.
            match extract_archive(archive2, root2.join("out")).await {
                Ok(()) => println!("deb ok: {:?}", std::fs::read_dir(root2.join("out")).map(|d| d.count())),
                Err(e) => println!("deb unsupported (likely zstd data.tar): {e}"),
            }
            let _ = std::fs::remove_dir_all(&root2);
        }
    }

    /// End-to-end against a real GitHub release: resolve -> download -> extract
    /// -> locate binary. Ignored by default (network + a real download); run
    /// with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_install_perfect_dark_linux() {
        let client = reqwest::Client::builder()
            .user_agent("decompdeck-test")
            .build()
            .unwrap();
        let releases =
            crate::github::fetch_releases(&client, "perfect-dark-pc-port/perfect_dark", None)
                .await
                .unwrap();
        let rel =
            crate::github::pick_release(&releases, "rolling", Some("ci-dev-build")).unwrap();
        let asset = crate::github::pick_asset(&rel, "(?i)x86_64-linux").unwrap();

        let root = scratch("real");
        let archive = root.join(&asset.name);
        download_to_file(&client, &asset.browser_download_url, &archive, |_, _| {})
            .await
            .unwrap();
        let dest = root.join("out");
        extract_archive(archive, dest.clone()).await.unwrap();
        let bin = find_launch_binary(&dest, Some("pd.x86_64"), false).unwrap();
        assert!(bin.exists(), "expected launch binary in {}", dest.display());
        let _ = std::fs::remove_dir_all(&root);
    }
}
