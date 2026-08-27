//! RetroAchievements account: logs in against the public `login2` endpoint and
//! returns the (user, token) pair. Freeport stores it centrally and hands it to
//! the in-game RA mod; it does NOT evaluate achievements itself.

use serde::Deserialize;

#[derive(Deserialize)]
struct LoginResp {
    #[serde(rename = "Success", default)]
    success: bool,
    #[serde(rename = "User", default)]
    user: Option<String>,
    #[serde(rename = "Token", default)]
    token: Option<String>,
    #[serde(rename = "Error", default)]
    error: Option<String>,
}

/// Logs in to RetroAchievements. Returns (canonical user, connect token).
/// The token (not the password) is what the RA client/mod uses afterwards.
pub async fn login(
    client: &reqwest::Client,
    user: &str,
    password: &str,
) -> Result<(String, String), String> {
    let url = format!(
        "https://retroachievements.org/dorequest.php?r=login2&u={}&p={}",
        urlencoding::encode(user),
        urlencoding::encode(password),
    );
    let resp: LoginResp = client
        .get(&url)
        .header("User-Agent", "Freeport/1.0 (freeport launcher)")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if resp.success {
        match (resp.user, resp.token) {
            (Some(u), Some(t)) if !t.is_empty() => Ok((u, t)),
            _ => Err("respuesta de RetroAchievements sin token".into()),
        }
    } else {
        Err(resp.error.unwrap_or_else(|| "usuario o contraseña incorrectos".into()))
    }
}

// ── Achievement progress (read-only, for the game page) ────────────────────
// Uses the same `dorequest.php` endpoints the in-game client uses, with the
// stored connect token — no separate Web API key needed.

#[derive(Debug, Clone)]
pub struct RaAchievement {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_name: String,
    pub unlocked: bool,
}

#[derive(Debug, Clone)]
pub struct RaProgress {
    pub game_id: u64,
    pub earned: usize,
    pub total: usize,
    pub points_earned: u32,
    pub points_total: u32,
    pub achievements: Vec<RaAchievement>,
}

/// Resolves the RA game id for a ROM file by MD5. The recomps require
/// big-endian `.z64` dumps, which is exactly the normalized form RA hashes
/// for N64, so a plain file MD5 matches.
pub async fn game_id_for_rom(client: &reqwest::Client, rom: &std::path::Path) -> Result<u64, String> {
    use md5::Digest as _;
    let data = tokio::fs::read(rom).await.map_err(|e| e.to_string())?;
    let hash = format!("{:x}", md5::Md5::digest(&data));
    let url = format!("https://retroachievements.org/dorequest.php?r=gameid&m={hash}");
    let v: serde_json::Value = client
        .get(&url)
        .header("User-Agent", "Freeport/1.0 (freeport launcher)")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    match v.get("GameID").and_then(|g| g.as_u64()) {
        Some(id) if id > 0 => Ok(id),
        _ => Err("ROM no reconocida por RetroAchievements".into()),
    }
}

/// Fetches the achievement list (core set) plus the user's softcore unlocks.
pub async fn fetch_progress(
    client: &reqwest::Client,
    user: &str,
    token: &str,
    game_id: u64,
) -> Result<RaProgress, String> {
    let get = |url: String| {
        let client = client.clone();
        async move {
            client
                .get(&url)
                .header("User-Agent", "Freeport/1.0 (freeport launcher)")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<serde_json::Value>()
                .await
                .map_err(|e| e.to_string())
        }
    };
    let (u, t) = (urlencoding::encode(user).into_owned(), urlencoding::encode(token).into_owned());
    let patch = get(format!(
        "https://retroachievements.org/dorequest.php?r=patch&u={u}&t={t}&g={game_id}"
    ))
    .await?;
    let unlocks = get(format!(
        "https://retroachievements.org/dorequest.php?r=unlocks&u={u}&t={t}&g={game_id}&h=0"
    ))
    .await?;

    let unlocked: std::collections::HashSet<u64> = unlocks
        .get("UserUnlocks")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_u64()).collect())
        .unwrap_or_default();

    let mut achievements: Vec<RaAchievement> = Vec::new();
    let list = patch
        .pointer("/PatchData/Achievements")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "respuesta de RA sin lista de logros".to_string())?;
    for a in list {
        // Flags 3 = core set; skip unofficial (5).
        if a.get("Flags").and_then(|f| f.as_u64()) != Some(3) {
            continue;
        }
        let id = a.get("ID").and_then(|v| v.as_u64()).unwrap_or(0);
        achievements.push(RaAchievement {
            id,
            title: a.get("Title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            description: a.get("Description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            points: a.get("Points").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            badge_name: a.get("BadgeName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            unlocked: unlocked.contains(&id),
        });
    }
    // Unlocked first, then by points ascending (RA's usual reading order).
    achievements.sort_by(|a, b| b.unlocked.cmp(&a.unlocked).then(a.points.cmp(&b.points)));
    let earned = achievements.iter().filter(|a| a.unlocked).count();
    let points_earned = achievements.iter().filter(|a| a.unlocked).map(|a| a.points).sum();
    let points_total = achievements.iter().map(|a| a.points).sum();
    Ok(RaProgress {
        game_id,
        earned,
        total: achievements.len(),
        points_earned,
        points_total,
        achievements,
    })
}

/// Badge art URL for an achievement (locked variant when not unlocked).
pub fn badge_url(badge_name: &str, unlocked: bool) -> String {
    if unlocked {
        format!("https://media.retroachievements.org/Badge/{badge_name}.png")
    } else {
        format!("https://media.retroachievements.org/Badge/{badge_name}_lock.png")
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Live check with the user's stored session (network + local config).
    #[tokio::test]
    #[ignore]
    async fn progress_for_mm_rom() {
        let cfg: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                dirs::data_dir().unwrap().join("decompdeck/config.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let (user, token) = (
            cfg["ra_user"].as_str().unwrap().to_string(),
            cfg["ra_token"].as_str().unwrap().to_string(),
        );
        let client = reqwest::Client::new();
        let rom = dirs::config_dir().unwrap().join("Zelda64Recompiled/mm.n64.us.1.0.z64");
        let id = game_id_for_rom(&client, &rom).await.expect("gameid");
        assert_eq!(id, 10679, "MM debería ser RA #10679");
        let p = fetch_progress(&client, &user, &token, id).await.expect("progress");
        println!("MM: {}/{} logros · {}/{} pts", p.earned, p.total, p.points_earned, p.points_total);
        assert!(p.total >= 90);
        assert!(!p.achievements[0].badge_name.is_empty());
    }
}
