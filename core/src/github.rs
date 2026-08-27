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
    /// Release notes / changelog (Markdown).
    #[serde(default)]
    pub body: Option<String>,
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
    let url = format!("https://api.github.com/repos/{slug}/releases?per_page=50");
    let req = apply_headers(client.get(&url), token);
    let resp = req.send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::msg(format!(
            "el repositorio {slug} no tiene releases publicadas"
        )));
    }
    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::msg(
            "GitHub limita su API anónima a 60 peticiones/hora por IP y se ha agotado. \
             Se renueva solo en menos de una hora; un token en Ajustes elimina el límite.",
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

/// Builds a `Release` for a known tag scraping the public `expanded_assets`
/// page instead of the REST API. The web pages are NOT subject to the 60
/// requests/hour per-IP limit of api.github.com, so installs keep working when
/// the anonymous API quota is gone. The tag normally comes from the catalog's
/// `cached.latest_tag` (refreshed daily by the probe CI, channel-aware), so
/// prerelease/draft filtering already happened upstream.
pub async fn fetch_release_by_tag_noapi(
    client: &reqwest::Client,
    slug: &str,
    tag: &str,
) -> AppResult<Release> {
    let url = format!("https://github.com/{slug}/releases/expanded_assets/{tag}");
    let resp = client
        .get(&url)
        .header("User-Agent", "decompdeck")
        .send()
        .await?
        .error_for_status()?;
    let html = resp.text().await?;
    let assets = parse_expanded_assets(&html);
    if assets.is_empty() {
        return Err(AppError::msg(format!(
            "la release {tag} de {slug} no tiene binarios descargables"
        )));
    }
    Ok(Release {
        tag_name: tag.to_string(),
        name: None,
        published_at: None,
        body: None,
        prerelease: false,
        draft: false,
        assets,
    })
}

fn parse_expanded_assets(html: &str) -> Vec<Asset> {
    let re = regex::Regex::new(r#"href="([^"]*/releases/download/[^"]+)""#).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let mut assets = Vec::new();
    for cap in re.captures_iter(html) {
        let href = cap[1].to_string();
        if !seen.insert(href.clone()) {
            continue;
        }
        let name = percent_decode(href.rsplit('/').next().unwrap_or_default());
        let url = if href.starts_with("http") {
            href
        } else {
            format!("https://github.com{href}")
        };
        assets.push(Asset { name, browser_download_url: url, size: 0 });
    }
    assets
}

/// Latest changelogs from the public `releases.atom` feed (no API quota).
/// Returns (tag, updated, plain-text body) per entry, newest first.
pub async fn fetch_changelogs_noapi(
    client: &reqwest::Client,
    slug: &str,
) -> AppResult<Vec<(String, Option<String>, String)>> {
    let url = format!("https://github.com/{slug}/releases.atom");
    let resp = client
        .get(&url)
        .header("User-Agent", "decompdeck")
        .send()
        .await?
        .error_for_status()?;
    let xml = resp.text().await?;
    let out = parse_releases_atom(&xml);
    if out.is_empty() {
        return Err(AppError::msg(format!("{slug} no tiene releases publicadas")));
    }
    Ok(out)
}

fn parse_releases_atom(xml: &str) -> Vec<(String, Option<String>, String)> {
    let tag_re = regex::Regex::new(r"/releases/tag/([^\x22<]+)").unwrap();
    let upd_re = regex::Regex::new(r"<updated>([^<]+)</updated>").unwrap();
    let body_re = regex::Regex::new(r"(?s)<content[^>]*>(.*?)</content>").unwrap();
    let mut out = Vec::new();
    for entry in xml.split("<entry>").skip(1) {
        let Some(tag) = tag_re.captures(entry).map(|c| percent_decode(&c[1])) else {
            continue;
        };
        let updated = upd_re.captures(entry).map(|c| c[1].to_string());
        let body = body_re
            .captures(entry)
            .map(|c| html_to_text(&xml_unescape(&c[1])))
            .unwrap_or_default();
        out.push((tag, updated, body));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Crude HTML → plain text for release notes coming from the atom feed.
fn html_to_text(html: &str) -> String {
    let block_re = regex::Regex::new(r"(?i)</(p|li|ul|ol|h[1-6]|div|tr|pre)>|<br\s*/?>").unwrap();
    let li_re = regex::Regex::new(r"(?i)<li[^>]*>").unwrap();
    let tag_re = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
    let text = block_re.replace_all(html, "\n");
    let text = li_re.replace_all(&text, "• ");
    let text = tag_re.replace_all(&text, "");
    let text = xml_unescape(&text);
    // Collapse runs of blank lines left over from the removed markup.
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
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
            body: None,
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
    fn expanded_assets_html_yields_assets() {
        let html = r#"
            <a href="/HarbourMasters/Starship/releases/download/v2.0.0/Starship-Barnard-Alfa-Linux.zip" rel="nofollow">x</a>
            <a href="/HarbourMasters/Starship/releases/download/v2.0.0/Starship-Barnard-Alfa-Linux.zip">dup</a>
            <a href="/HarbourMasters/Starship/releases/download/v2.0.0/Some%20Name.zip">enc</a>
        "#;
        let assets = parse_expanded_assets(html);
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].name, "Starship-Barnard-Alfa-Linux.zip");
        assert!(assets[0].browser_download_url.starts_with("https://github.com/"));
        assert_eq!(assets[1].name, "Some Name.zip");
    }

    #[test]
    fn atom_feed_yields_tag_date_and_text_body() {
        let xml = r#"<feed><entry>
            <id>tag:github.com,2008:Repository/1/v1.2</id>
            <updated>2026-08-01T00:00:00Z</updated>
            <link href="https://github.com/o/r/releases/tag/v1.2"/>
            <content type="html">&lt;p&gt;Fixes &amp;amp; stuff&lt;/p&gt;&lt;ul&gt;&lt;li&gt;one&lt;/li&gt;&lt;/ul&gt;</content>
        </entry></feed>"#;
        let entries = parse_releases_atom(xml);
        assert_eq!(entries.len(), 1);
        let (tag, updated, body) = &entries[0];
        assert_eq!(tag, "v1.2");
        assert_eq!(updated.as_deref(), Some("2026-08-01T00:00:00Z"));
        assert!(body.contains("Fixes & stuff"), "body: {body:?}");
        assert!(body.contains("• one"), "body: {body:?}");
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

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Live check of the quota-free paths against real GitHub (network).
    #[tokio::test]
    #[ignore]
    async fn noapi_paths_work_against_real_github() {
        let client = reqwest::Client::new();
        let rel = fetch_release_by_tag_noapi(&client, "HarbourMasters/Starship", "v2.0.0")
            .await
            .expect("expanded_assets");
        assert!(pick_asset(&rel, "(?i)linux\\.zip$").is_ok(), "assets: {:?}",
            rel.assets.iter().map(|a| &a.name).collect::<Vec<_>>());
        let logs = fetch_changelogs_noapi(&client, "Zelda64Recomp/Zelda64Recomp")
            .await
            .expect("releases.atom");
        assert!(!logs[0].0.is_empty() && !logs[0].2.is_empty());
    }
}
