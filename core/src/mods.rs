use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A mod as shown to the frontend (from a Thunderstore community's latest
/// version of each package).
#[derive(Serialize, Clone)]
pub struct ModInfo {
    pub full_name: String, // "Owner-Name"
    pub name: String,
    pub owner: String,
    pub description: String,
    pub version: String,
    pub download_url: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub dependencies: Vec<String>,
    pub package_url: Option<String>,
}

#[derive(Deserialize)]
struct TsPackage {
    name: String,
    full_name: String,
    owner: String,
    #[serde(default)]
    package_url: Option<String>,
    #[serde(default)]
    is_deprecated: bool,
    versions: Vec<TsVersion>,
}

#[derive(Deserialize)]
struct TsVersion {
    #[serde(default)]
    version_number: String,
    download_url: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    dependencies: Vec<String>,
}

/// Fetches the mod list for a Thunderstore community, newest version of each
/// package, sorted by popularity.
pub async fn fetch_mods(client: &reqwest::Client, community: &str) -> AppResult<Vec<ModInfo>> {
    let url = format!("https://thunderstore.io/c/{community}/api/v1/package/");
    let resp = client
        .get(&url)
        .header("User-Agent", "DecompDeck")
        .send()
        .await?
        .error_for_status()?;
    let pkgs: Vec<TsPackage> = resp.json().await?;
    let mut out = Vec::new();
    for p in pkgs {
        if p.is_deprecated {
            continue;
        }
        let Some(v) = p.versions.into_iter().next() else {
            continue;
        };
        out.push(ModInfo {
            full_name: p.full_name,
            name: p.name,
            owner: p.owner,
            description: v.description,
            version: v.version_number,
            download_url: v.download_url,
            icon_url: v.icon,
            downloads: v.downloads,
            dependencies: v.dependencies,
            package_url: p.package_url,
        });
    }
    out.sort_by_key(|m| std::cmp::Reverse(m.downloads));
    Ok(out)
}

/// "Owner-Name-1.2.3" -> "Owner-Name" (Thunderstore dependency string).
fn strip_version(dep: &str) -> String {
    match dep.rsplit_once('-') {
        Some((base, _ver)) => base.to_string(),
        None => dep.to_string(),
    }
}

/// Extracts the `.nrm` mod file(s) from a zip on disk into `mods_dir` (streamed
/// from the file, so multi-GB HD texture packs don't blow up memory).
/// Extracts payload files from a zip on disk into `mods_dir`. If `allowed` is
/// Some, only files with those extensions are kept (e.g. GameBanana → o2r/otr);
/// if None, everything except packaging metadata is kept (Thunderstore).
async fn extract_payload(
    archive: std::path::PathBuf,
    mods_dir: std::path::PathBuf,
    allowed: Option<Vec<String>>,
) -> AppResult<Vec<String>> {
    tokio::task::spawn_blocking(move || -> AppResult<Vec<String>> {
        std::fs::create_dir_all(&mods_dir)?;
        let file = std::fs::File::open(&archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        let mut names = Vec::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            let name = f.name().to_string();
            if name.ends_with('/') {
                continue; // directory entry
            }
            let base = Path::new(&name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if base.is_empty() {
                continue;
            }
            let bl = base.to_lowercase();
            let keep = match &allowed {
                Some(exts) => exts.iter().any(|e| bl.ends_with(&format!(".{e}"))),
                None => {
                    // Skip packaging metadata; install everything else.
                    let is_meta = matches!(
                        bl.as_str(),
                        "manifest.json" | "mod.json" | "icon.png" | "readme.md" | "changelog.md"
                    ) || bl.ends_with(".md")
                        || bl.ends_with(".txt");
                    !is_meta
                }
            };
            if !keep {
                continue;
            }
            let mut out = std::fs::File::create(mods_dir.join(&base))?;
            std::io::copy(&mut f, &mut out)?;
            names.push(base);
        }
        Ok(names)
    })
    .await
    .map_err(|e| AppError::msg(format!("fallo al extraer el mod: {e}")))?
}

/// Installs a mod and its dependencies (resolved within the same community),
/// reporting progress via `on_progress(pkg, index, total, downloaded, bytes, phase)`.
/// Returns the list of `.nrm` files written.
pub async fn install_mod(
    client: &reqwest::Client,
    all: &[ModInfo],
    full_name: &str,
    mods_dir: &Path,
    mut on_progress: impl FnMut(&str, usize, usize, u64, u64, &str),
) -> AppResult<Vec<String>> {
    let by_name: std::collections::HashMap<&str, &ModInfo> =
        all.iter().map(|m| (m.full_name.as_str(), m)).collect();

    // Resolve the full install set (target + dependencies) up front so we know
    // the package count for the progress bar.
    let mut order: Vec<ModInfo> = Vec::new();
    let mut visited = std::collections::BTreeSet::new();
    let mut queue = vec![full_name.to_string()];
    while let Some(fname) = queue.pop() {
        if !visited.insert(fname.clone()) {
            continue;
        }
        if let Some(m) = by_name.get(fname.as_str()) {
            order.push((*m).clone());
            for dep in &m.dependencies {
                queue.push(strip_version(dep));
            }
        }
    }

    let total = order.len();
    let mut installed = Vec::new();
    std::fs::create_dir_all(mods_dir)?;
    // Unique temp prefix per clicked mod so parallel installs never share a
    // download file (that caused the "no such file/directory" errors).
    let slug: String = full_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    for (i, m) in order.iter().enumerate() {
        let name = m.name.clone();
        // Stream to a temp file on disk (handles multi-GB HD texture packs).
        let tmp = mods_dir.join(format!(".dd-download-{slug}-{i}.zip"));
        crate::install::download_to_file(client, &m.download_url, &tmp, |d, t| {
            on_progress(&name, i + 1, total, d, t, "download")
        })
        .await?;
        on_progress(&name, i + 1, total, 0, 0, "extract");
        let files = extract_payload(tmp.clone(), mods_dir.to_path_buf(), None).await?;
        let _ = std::fs::remove_file(&tmp);
        installed.extend(files);
    }
    on_progress("", total, total, 0, 0, "done");

    if installed.is_empty() {
        return Err(AppError::msg(
            "el paquete no contenía ningún archivo de mod (.nrm)",
        ));
    }
    Ok(installed)
}

// ---------------------------------------------------------------------------
// GameBanana (Ship of Harkinian & HarbourMasters ecosystem)
// ---------------------------------------------------------------------------

/// Lists mods for a GameBanana game id (a couple of subfeed pages).
pub async fn fetch_gb_mods(client: &reqwest::Client, game_id: &str) -> AppResult<Vec<ModInfo>> {
    let mut out = Vec::new();
    for page in 1..=3 {
        let url = format!(
            "https://gamebanana.com/apiv11/Game/{game_id}/Subfeed?_nPage={page}&_sSort=default&_csvModelInclusions=Mod"
        );
        let v: serde_json::Value = client
            .get(&url)
            .header("User-Agent", "DecompDeck")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let recs = v
            .get("_aRecords")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        if recs.is_empty() {
            break;
        }
        for r in recs {
            if r.get("_sModelName").and_then(|s| s.as_str()) != Some("Mod") {
                continue;
            }
            if r.get("_bHasFiles").and_then(|b| b.as_bool()) == Some(false) {
                continue;
            }
            let id = r.get("_idRow").and_then(|x| x.as_i64()).unwrap_or(0);
            if id == 0 {
                continue;
            }
            let icon = r
                .get("_aPreviewMedia")
                .and_then(|p| p.get("_aImages"))
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|img| {
                    let base = img.get("_sBaseUrl").and_then(|s| s.as_str())?;
                    let file = img.get("_sFile").and_then(|s| s.as_str())?;
                    Some(format!("{base}/{file}"))
                });
            out.push(ModInfo {
                full_name: format!("gb-{id}"),
                name: r.get("_sName").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                owner: r
                    .get("_aSubmitter")
                    .and_then(|s| s.get("_sName"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: String::new(),
                version: r
                    .get("_tsDateUpdated")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0)
                    .to_string(),
                download_url: String::new(),
                icon_url: icon,
                downloads: r.get("_nViewCount").and_then(|x| x.as_u64()).unwrap_or(0),
                dependencies: vec![],
                package_url: r.get("_sProfileUrl").and_then(|s| s.as_str()).map(String::from),
            });
        }
    }
    Ok(out)
}

/// Downloads a GameBanana mod (largest file) and installs its `.o2r`/`.otr`
/// payload into `mods_dir` (next to the SoH-style executable).
pub async fn install_gb_mod(
    client: &reqwest::Client,
    full_name: &str,
    mods_dir: &Path,
    mut on_progress: impl FnMut(&str, usize, usize, u64, u64, &str),
) -> AppResult<Vec<String>> {
    let id = full_name
        .strip_prefix("gb-")
        .ok_or_else(|| AppError::msg("id de GameBanana inválido"))?;
    let url = format!("https://gamebanana.com/apiv11/Mod/{id}?_csvProperties=_sName,_aFiles");
    let v: serde_json::Value = client
        .get(&url)
        .header("User-Agent", "DecompDeck")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let files = v.get("_aFiles").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let file = files
        .iter()
        .max_by_key(|f| f.get("_nFilesize").and_then(|x| x.as_u64()).unwrap_or(0))
        .ok_or_else(|| AppError::msg("el mod no tiene archivos descargables"))?;
    let dl = file
        .get("_sDownloadUrl")
        .and_then(|s| s.as_str())
        .ok_or_else(|| AppError::msg("el mod no tiene URL de descarga"))?;
    let fname = file.get("_sFile").and_then(|s| s.as_str()).unwrap_or("mod.zip").to_string();
    let name = v.get("_sName").and_then(|s| s.as_str()).unwrap_or("mod").to_string();
    std::fs::create_dir_all(mods_dir)?;
    let lower = fname.to_lowercase();

    if lower.ends_with(".zip") {
        let tmp = mods_dir.join(format!(".dd-gb-{id}.zip"));
        crate::install::download_to_file(client, dl, &tmp, |d, t| {
            on_progress(&name, 1, 1, d, t, "download")
        })
        .await?;
        on_progress(&name, 1, 1, 0, 0, "extract");
        let installed =
            extract_payload(tmp.clone(), mods_dir.to_path_buf(), Some(vec!["o2r".into(), "otr".into()]))
                .await?;
        let _ = std::fs::remove_file(&tmp);
        on_progress("", 1, 1, 0, 0, "done");
        if installed.is_empty() {
            return Err(AppError::msg(
                "el archivo no contenía mods .o2r/.otr (¿es un pack manual?)",
            ));
        }
        Ok(installed)
    } else if lower.ends_with(".o2r") || lower.ends_with(".otr") {
        let dest = mods_dir.join(&fname);
        crate::install::download_to_file(client, dl, &dest, |d, t| {
            on_progress(&name, 1, 1, d, t, "download")
        })
        .await?;
        on_progress("", 1, 1, 0, 0, "done");
        Ok(vec![fname])
    } else {
        Err(AppError::msg(format!(
            "formato no soportado ({fname}): solo .zip con .o2r/.otr o archivos .o2r/.otr sueltos. Los .7z/.rar hay que instalarlos a mano."
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn real_install_small_mod() {
        let client = reqwest::Client::builder().user_agent("DecompDeck").build().unwrap();
        let all = fetch_mods(&client, "zelda-64-recompiled").await.unwrap();
        // smallest by picking one with a small file — just take a well-known tiny lib.
        let target = all
            .iter()
            .min_by_key(|m| m.downloads) // arbitrary small pick
            .map(|m| m.full_name.clone())
            .unwrap();
        // use a real popular one instead to ensure .nrm present:
        let target = all.iter().find(|m| m.full_name.contains("Audio_API")).map(|m| m.full_name.clone()).unwrap_or(target);
        let dir = std::env::temp_dir().join(format!("dd-mods-test-{}", std::process::id())).join("mods");
        println!("mods_dir = {}", dir.display());
        let res = install_mod(&client, &all, &target, &dir, |pkg, i, t, d, tot, ph| {
            if ph == "extract" || d == 0 { println!("  {ph} {pkg} {i}/{t} ({d}/{tot})"); }
        }).await;
        match &res {
            Ok(files) => println!("OK instalados: {files:?} en {}", dir.display()),
            Err(e) => println!("ERROR: {e}"),
        }
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        res.unwrap();
    }
}
