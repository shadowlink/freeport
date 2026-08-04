use crate::error::{AppError, AppResult};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    #[serde(default)]
    pub tag_name: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub name: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub size: u64,
}

fn apply_headers(mut req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    // GitHub requires a User-Agent; the API version header is recommended.
    req = req
        .header("User-Agent", "decompdeck")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        if !t.is_empty() {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
    }
    req
}

/// Lists recent releases for a repo. Uses the *list* endpoint (not
/// `/releases/latest`) because projects that publish only prereleases return
/// 404 on `/latest`.
pub async fn fetch_releases(
    client: &reqwest::Client,
    slug: &str,
    token: Option<&str>,
) -> AppResult<Vec<Release>> {
    let url = format!("https://api.github.com/repos/{slug}/releases?per_page=10");
    let req = apply_headers(client.get(&url), token);
    let resp = req.send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::msg(format!(
            "el repositorio {slug} no tiene releases publicadas"
        )));
    }
    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::msg(
            "GitHub devolvió 403 (posible límite de peticiones). Añade un token en Ajustes.",
        ));
    }
    if !resp.status().is_success() {
        return Err(AppError::msg(format!(
            "GitHub respondió {} para {slug}",
            resp.status()
        )));
    }
    let releases: Vec<Release> = resp.json().await?;
    Ok(releases)
}

/// Picks the release matching a project's channel:
/// - "stable": newest non-draft, non-prerelease
/// - "prerelease": newest non-draft (prereleases allowed)
/// - "rolling": the one whose tag equals `rolling_tag`
pub fn pick_release(
    releases: &[Release],
    channel: &str,
    rolling_tag: Option<&str>,
) -> Option<Release> {
    match channel {
        "rolling" => {
            if let Some(tag) = rolling_tag {
                releases.iter().find(|r| r.tag_name == tag).cloned()
            } else {
                releases.iter().find(|r| !r.draft).cloned()
            }
        }
        "prerelease" => releases.iter().find(|r| !r.draft).cloned(),
        _ => releases
            .iter()
            .find(|r| !r.draft && !r.prerelease)
            .cloned()
            // fall back to any non-draft release if none are marked stable
            .or_else(|| releases.iter().find(|r| !r.draft).cloned()),
    }
}

/// Finds the asset in a release whose name matches the given regex.
pub fn pick_asset(release: &Release, regex_str: &str) -> AppResult<Asset> {
    let re = regex::Regex::new(regex_str)
        .map_err(|e| AppError::msg(format!("regex de asset inválida: {e}")))?;
    release
        .assets
        .iter()
        .find(|a| re.is_match(&a.name))
        .cloned()
        .ok_or_else(|| {
            AppError::msg(format!(
                "no se encontró un binario que case con «{regex_str}» en la release {}",
                release.tag_name
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, prerelease: bool, draft: bool, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.into(),
            name: None,
            published_at: Some("2026-01-01".into()),
            prerelease,
            draft,
            assets: assets
                .iter()
                .map(|n| Asset {
                    name: (*n).into(),
                    browser_download_url: format!("https://example/{n}"),
                    size: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn stable_skips_prerelease_and_draft() {
        let releases = vec![
            rel("v3-draft", false, true, &[]),
            rel("v2-pre", true, false, &[]),
            rel("v1", false, false, &[]),
        ];
        let picked = pick_release(&releases, "stable", None).unwrap();
        assert_eq!(picked.tag_name, "v1");
    }

    #[test]
    fn rolling_matches_fixed_tag() {
        let releases = vec![
            rel("nightly", false, false, &[]),
            rel("ci-dev-build", true, false, &[]),
        ];
        let picked = pick_release(&releases, "rolling", Some("ci-dev-build")).unwrap();
        assert_eq!(picked.tag_name, "ci-dev-build");
    }

    #[test]
    fn asset_regex_picks_linux_over_flatpak_and_arm() {
        // Mirrors the real Zelda64Recomp release layout.
        let r = rel(
            "v1.2.2",
            false,
            false,
            &[
                "Zelda64Recompiled-v1.2.2-Windows.zip",
                "Zelda64Recompiled-v1.2.2-Linux-X64.zip",
                "Zelda64Recompiled-v1.2.2-Linux-ARM64.zip",
                "Zelda64Recompiled-v1.2.2-Linux-Flatpak-X64.zip",
            ],
        );
        let a = pick_asset(&r, "(?i)linux-x64").unwrap();
        assert_eq!(a.name, "Zelda64Recompiled-v1.2.2-Linux-X64.zip");
    }
}
