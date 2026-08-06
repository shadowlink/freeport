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
