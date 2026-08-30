use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::CliError;

const CLERK_BASE: &str = "https://auth.suno.com";
const CLERK_JS_VERSION: &str = "5.117.0";
const CLERK_API_VERSION: &str = "2025-11-10";

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct AuthState {
    pub jwt: Option<String>,
    pub cookie: Option<String>,
    pub session_id: Option<String>,
    pub device_id: Option<String>,
    /// The __client cookie from clerk domain — long-lived (~7 days)
    pub clerk_client_cookie: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BrowserAuth {
    pub clerk_client_cookie: String,
    pub cookie_header: String,
    pub device_id: Option<String>,
}

impl AuthState {
    pub fn load() -> Result<Self, CliError> {
        let path = Self::path();
        if !path.exists() {
            return Err(CliError::AuthMissing);
        }
        let data = std::fs::read_to_string(&path)?;
        serde_json::from_str(&data).map_err(|e| CliError::Config(format!("corrupt auth file: {e}")))
    }

    pub fn save(&self) -> Result<(), CliError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;

        // Atomic write: create temp file with restricted permissions, then rename
        let tmp = path.with_extension("json.tmp");

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(data.as_bytes())?;
            file.sync_all()?;
        }

        #[cfg(not(unix))]
        {
            std::fs::write(&tmp, &data)?;
        }

        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn delete() -> Result<(), CliError> {
        let path = Self::path();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn is_jwt_expired(&self) -> bool {
        let Some(jwt) = &self.jwt else { return true };
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            return true;
        }
        let claims = parts[1];
        // JWT claims use Base64URL encoding, not standard Base64
        let Ok(decoded) = BASE64URL.decode(claims) else {
            return true;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&decoded) else {
            return true;
        };
        let Some(exp) = value.get("exp").and_then(|v| v.as_u64()) else {
            return true;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Refresh aggressively: any JWT with under 30 minutes of life left.
        //
        // Suno issues 1-hour JWTs but their generation endpoint silently
        // rejects tokens older than ~30 minutes with `Token validation
        // failed.` even when the JWT's own `exp` claim says it's still
        // valid (verified 2026-04-07). The 30-minute threshold ensures we
        // always hand the API a freshly-minted JWT.
        now + 1800 >= exp
    }

    fn path() -> PathBuf {
        crate::config::config_dir().join("auth.json")
    }
}

fn strip_cookie_header_prefix(input: &str) -> &str {
    let trimmed = input.trim();
    if trimmed.len() >= "cookie:".len()
        && trimmed[.."cookie:".len()].eq_ignore_ascii_case("cookie:")
    {
        trimmed["cookie:".len()..].trim()
    } else {
        trimmed
    }
}

fn parse_cookie_header(input: &str) -> HashMap<String, String> {
    strip_cookie_header_prefix(input)
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), value.trim().to_string()))
        })
        .collect()
}

fn sanitize_device_id(value: &str) -> Option<String> {
    let sanitized = value
        .trim()
        .replace("%22", "\"")
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string();
    if sanitized.is_empty() || sanitized.contains(';') {
        None
    } else {
        Some(sanitized)
    }
}

pub fn normalize_cookie_input(input: &str) -> Result<BrowserAuth, CliError> {
    let normalized = strip_cookie_header_prefix(input);
    let cookies = parse_cookie_header(normalized);

    if let Some(clerk_client_cookie) = cookies.get("__client").filter(|v| !v.is_empty()) {
        let device_id = cookies
            .get("ajs_anonymous_id")
            .and_then(|v| sanitize_device_id(v));
        return Ok(BrowserAuth {
            clerk_client_cookie: clerk_client_cookie.clone(),
            cookie_header: normalized.to_string(),
            device_id,
        });
    }

    if normalized.contains(';') || normalized.contains('=') {
        return Err(CliError::Config(
            "cookie header did not contain a __client field".into(),
        ));
    }

    let clerk_client_cookie = normalized.trim().to_string();
    if clerk_client_cookie.is_empty() {
        return Err(CliError::Config("empty Clerk __client cookie".into()));
    }
    Ok(BrowserAuth {
        cookie_header: format!("__client={clerk_client_cookie}"),
        clerk_client_cookie,
        device_id: None,
    })
}

fn clerk_client_url() -> String {
    format!(
        "{CLERK_BASE}/v1/client?__clerk_api_version={CLERK_API_VERSION}&_clerk_js_version={CLERK_JS_VERSION}"
    )
}

fn clerk_token_url(session_id: &str) -> String {
    format!(
        "{CLERK_BASE}/v1/client/sessions/{session_id}/tokens?__clerk_api_version={CLERK_API_VERSION}&_clerk_js_version={CLERK_JS_VERSION}"
    )
}

fn apply_clerk_headers(
    builder: reqwest::RequestBuilder,
    clerk_cookie: &str,
) -> reqwest::RequestBuilder {
    builder
        .header("authorization", clerk_cookie)
        .header("cookie", format!("__client={clerk_cookie}"))
        .header("origin", "https://suno.com")
        .header("referer", "https://suno.com/")
}

fn response_excerpt(body: &str) -> String {
    const MAX: usize = 500;
    let body = body.replace(['\n', '\r'], " ");
    if body.len() <= MAX {
        body
    } else {
        format!("{}...", body.chars().take(MAX).collect::<String>())
    }
}

/// Generate the dynamic browser-token header value.
pub fn browser_token() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let payload = format!(r#"{{"timestamp":{ms}}}"#);
    let encoded = BASE64.encode(payload.as_bytes());
    format!(r#"{{"token":"{encoded}"}}"#)
}

/// Chromium-family browsers we scan, in preference order, as
/// (rookie config key, display name). Every key must exist in rookie's
/// per-platform config table — `get_browser_config` unwraps internally, so an
/// unknown key panics rather than returning an error.
const CHROMIUM_BROWSERS: &[(&str, &str)] = &[
    ("chrome", "Chrome"),
    ("arc", "Arc"),
    ("brave", "Brave"),
    ("edge", "Edge"),
    ("chromium", "Chromium"),
    ("vivaldi", "Vivaldi"),
    ("opera", "Opera"),
];

#[cfg(unix)]
fn expand_browser_path(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) => path.replace("$HOME", &home).replace('~', &home),
        Err(_) => path.to_string(),
    }
}

#[cfg(windows)]
fn expand_browser_path(path: &str) -> String {
    // Expand %VAR% the way rookie does, leaving unset names untouched.
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => out.push_str(&format!("%{name}%")),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Human-readable profile name for a cookie DB path.
/// `.../Profile 7/Cookies` and `.../Profile 7/Network/Cookies` both give
/// "Profile 7".
fn profile_label(db: &Path) -> String {
    let mut dir = db.parent();
    if dir.and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new("Network")) {
        dir = dir.and_then(|p| p.parent());
    }
    dir.and_then(|p| p.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| db.display().to_string())
}

/// Every cookie DB rookie knows about for `browser`, most recently written
/// first.
///
/// rookie's own `chrome()` helper returns the *first* profile its glob matches
/// and stops there. On a Chrome install whose profiles are `Profile 5`..
/// `Profile 9` with no `Default`, that is `Profile 5` — a session living in
/// `Profile 7` is invisible, and the user just sees "no session found" after
/// unlocking their keychain. Expanding every match and trying them all is the
/// only way to reach a non-default profile.
fn chromium_cookie_dbs(browser: &str) -> Vec<PathBuf> {
    let config = rookie::config::get_browser_config(browser);
    let channels = config
        .channels
        .clone()
        .unwrap_or_else(|| vec![String::new()]);

    let mut dbs: Vec<PathBuf> = Vec::new();
    for pattern in &config.paths {
        for channel in &channels {
            let expanded = expand_browser_path(&pattern.replace("{channel}", channel));
            let Ok(matches) = glob::glob(&expanded) else {
                continue;
            };
            for db in matches.flatten() {
                if db.is_file() && !dbs.contains(&db) {
                    dbs.push(db);
                }
            }
        }
    }

    // Newest first: when several profiles hold a __client, the one the user
    // actually browses Suno in is the one that was written most recently.
    dbs.sort_by_key(|db| {
        std::cmp::Reverse(
            std::fs::metadata(db)
                .and_then(|meta| meta.modified())
                .unwrap_or(UNIX_EPOCH),
        )
    });
    dbs
}

/// The `Local State` file holding the DPAPI-wrapped key, relative to a
/// Windows cookie DB.
#[cfg(windows)]
fn chromium_key_path(db: &Path) -> Option<PathBuf> {
    let parent = db.parent()?;
    ["../../Local State", "../Local State", "Local State"]
        .iter()
        .map(|candidate| parent.join(candidate))
        .find(|candidate| candidate.exists())
}

fn read_chromium_cookies(
    browser: &str,
    db: &Path,
    domains: &[String],
) -> Result<Vec<rookie::enums::Cookie>, String> {
    #[cfg(windows)]
    {
        let _ = browser;
        let key = chromium_key_path(db)
            .ok_or_else(|| "no Local State file beside the cookie database".to_string())?;
        rookie::chromium_based(key, db.to_path_buf(), Some(domains.to_vec()))
            .map_err(|e| e.to_string())
    }
    #[cfg(unix)]
    {
        let config = rookie::config::get_browser_config(browser);
        rookie::chromium_based(config, db.to_path_buf(), Some(domains.to_vec()))
            .map_err(|e| e.to_string())
    }
}

/// Pull the Clerk session out of one browser profile's cookie jar.
/// Returns `None` when the profile has no Suno `__client` cookie.
fn browser_auth_from_cookies(cookies: Vec<rookie::enums::Cookie>) -> Option<BrowserAuth> {
    let mut seen = HashSet::new();
    let mut header_parts = Vec::new();
    let mut clerk_client_cookie: Option<String> = None;
    let mut auth_domain_clerk: Option<String> = None;
    let mut device_id: Option<String> = None;

    for cookie in cookies {
        if !cookie.domain.contains("suno.com") {
            continue;
        }
        if cookie.name == "__client" && !cookie.value.is_empty() {
            if cookie.domain.contains("auth.suno.com") {
                auth_domain_clerk = Some(cookie.value.clone());
            } else if clerk_client_cookie.is_none() {
                clerk_client_cookie = Some(cookie.value.clone());
            }
        }
        if cookie.name == "ajs_anonymous_id" && device_id.is_none() {
            device_id = sanitize_device_id(&cookie.value);
        }
        let key = (cookie.name.clone(), cookie.domain.clone());
        if seen.insert(key) {
            header_parts.push(format!("{}={}", cookie.name, cookie.value));
        }
    }

    Some(BrowserAuth {
        clerk_client_cookie: auth_domain_clerk.or(clerk_client_cookie)?,
        cookie_header: header_parts.join("; "),
        device_id,
    })
}

/// Extract Suno auth cookies from the user's browsers.
/// Scans every profile of every Chromium-family browser, then Firefox, then
/// Safari on macOS.
pub fn extract_browser_auth() -> Result<BrowserAuth, CliError> {
    let domains = vec![
        "suno.com".to_string(),
        "auth.suno.com".to_string(),
        ".suno.com".to_string(),
    ];

    // Profiles we opened but that held no Suno session, and profiles we could
    // not read at all. Both go into the failure message: a silent
    // "no session found" is the single most confusing way this can fail.
    let mut scanned: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for (key, name) in CHROMIUM_BROWSERS {
        for db in chromium_cookie_dbs(key) {
            let profile = profile_label(&db);
            match read_chromium_cookies(key, &db, &domains) {
                Ok(cookies) => match browser_auth_from_cookies(cookies) {
                    Some(auth) => {
                        eprintln!("Found Suno session in {name} ({profile})");
                        return Ok(auth);
                    }
                    None => scanned.push(format!("{name} ({profile})")),
                },
                Err(e) => failures.push(format!("{name} ({profile}): {e}")),
            }
        }
    }

    // Firefox and Safari keep one store per install rather than per profile
    // directory, so rookie's own lookup is enough.
    let mut others: Vec<(&str, rookie::Result<Vec<rookie::enums::Cookie>>)> =
        vec![("Firefox", rookie::firefox(Some(domains.clone())))];
    #[cfg(target_os = "macos")]
    others.push(("Safari", rookie::safari(Some(domains.clone()))));

    for (name, result) in others {
        match result {
            Ok(cookies) => match browser_auth_from_cookies(cookies) {
                Some(auth) => {
                    eprintln!("Found Suno session in {name}");
                    return Ok(auth);
                }
                None => scanned.push(name.to_string()),
            },
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }

    let mut message =
        String::from("No Suno session found in any browser. Log into suno.com first, then retry.");
    if !scanned.is_empty() {
        message.push_str(&format!(
            "\n  Scanned, no Suno cookies: {}",
            scanned.join(", ")
        ));
    }
    if !failures.is_empty() {
        message.push_str(&format!("\n  Could not read: {}", failures.join("; ")));
    }
    message.push_str(
        "\n  Manual fallback: copy the Cookie header from suno.com in DevTools, \
         then run `suno auth --cookie '<cookie header>'`",
    );
    Err(CliError::Config(message))
}

/// Exchange the __client cookie for a session ID and JWT via Clerk.
pub async fn clerk_token_exchange(
    client: &reqwest::Client,
    clerk_cookie: &str,
) -> Result<(String, String), CliError> {
    // Step 1: Get session ID
    let resp = apply_clerk_headers(client.get(clerk_client_url()), clerk_cookie)
        .send()
        .await
        .map_err(CliError::Http)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CliError::Api {
            code: "clerk_exchange_failed",
            message: format!(
                "Clerk token exchange failed ({status}): {}",
                response_excerpt(&body)
            ),
        });
    }

    let body: serde_json::Value = resp.json().await.map_err(CliError::Http)?;
    let session_id = body
        .get("response")
        .and_then(|r| {
            r.get("last_active_session_id")
                .and_then(|s| s.as_str())
                .or_else(|| {
                    r.get("sessions")
                        .and_then(|s| s.as_array())
                        .and_then(|sessions| sessions.first())
                        .and_then(|session| session.get("id"))
                        .and_then(|id| id.as_str())
                })
        })
        .ok_or_else(|| CliError::Api {
            code: "no_session",
            message: "No active session found — log into suno.com in your browser first".into(),
        })?
        .to_string();

    // Step 2: Exchange for JWT
    let jwt = clerk_refresh_jwt(client, clerk_cookie, &session_id).await?;

    Ok((session_id, jwt))
}

/// Refresh JWT using stored Clerk cookie + session ID.
pub async fn clerk_refresh_jwt(
    client: &reqwest::Client,
    clerk_cookie: &str,
    session_id: &str,
) -> Result<String, CliError> {
    let resp = apply_clerk_headers(client.post(clerk_token_url(session_id)), clerk_cookie)
        .header("content-type", "application/x-www-form-urlencoded")
        .send()
        .await
        .map_err(CliError::Http)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CliError::Api {
            code: "clerk_refresh_failed",
            message: format!(
                "Clerk JWT refresh failed ({status}): {}",
                response_excerpt(&body)
            ),
        });
    }

    let body: serde_json::Value = resp.json().await.map_err(CliError::Http)?;
    body.get("jwt")
        .and_then(|j| j.as_str())
        .map(String::from)
        .ok_or_else(|| CliError::Api {
            code: "no_jwt",
            message:
                "Clerk returned no JWT — session may have expired, run `suno auth login` again"
                    .into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_raw_client_cookie() {
        let auth = normalize_cookie_input("client_token").unwrap();
        assert_eq!(auth.clerk_client_cookie, "client_token");
        assert_eq!(auth.cookie_header, "__client=client_token");
        assert!(auth.device_id.is_none());
    }

    #[test]
    fn normalizes_full_cookie_header_and_device() {
        let auth = normalize_cookie_input(
            "Cookie: foo=bar; __client=client_token; ajs_anonymous_id=%22device-123%22",
        )
        .unwrap();
        assert_eq!(auth.clerk_client_cookie, "client_token");
        assert_eq!(auth.device_id.as_deref(), Some("device-123"));
        assert!(auth.cookie_header.contains("__client=client_token"));
    }

    #[test]
    fn rejects_cookie_header_without_client() {
        let err = normalize_cookie_input("foo=bar; ajs_anonymous_id=device").unwrap_err();
        assert!(err.to_string().contains("__client"));
    }
}
