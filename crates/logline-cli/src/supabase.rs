use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};

// ─── Config ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupabaseConfig {
    pub url: String,
    pub anon_key: String,
}

impl SupabaseConfig {
    pub fn from_env_or_file() -> anyhow::Result<Self> {
        if let (Ok(url), Ok(key)) = (
            std::env::var("NEXT_PUBLIC_SUPABASE_URL"),
            std::env::var("NEXT_PUBLIC_SUPABASE_ANON_KEY"),
        ) {
            if !url.is_empty() && !key.is_empty() {
                return Ok(Self { url, anon_key: key });
            }
        }

        let config_path = config_dir().join("config.json");
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .context("Failed to read config.json")?;
            return serde_json::from_str(&content)
                .context("Invalid config.json format");
        }

        for filename in [".env.local", ".env"] {
            let path = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(filename);
            if let Ok(content) = fs::read_to_string(&path) {
                let url = parse_env_value(&content, "NEXT_PUBLIC_SUPABASE_URL");
                let key = parse_env_value(&content, "NEXT_PUBLIC_SUPABASE_ANON_KEY");
                if let (Some(url), Some(key)) = (url, key) {
                    return Ok(Self { url, anon_key: key });
                }
            }
        }

        bail!(
            "Supabase config not found.\n\
             Set NEXT_PUBLIC_SUPABASE_URL and NEXT_PUBLIC_SUPABASE_ANON_KEY env vars,\n\
             or create ~/.config/logline/config.json with {{\"url\": \"...\", \"anon_key\": \"...\"}}"
        )
    }
}

// ─── Stored auth tokens ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredAuth {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub auth_method: Option<String>,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("logline")
}

pub fn auth_path() -> PathBuf {
    config_dir().join("auth.json")
}

const KEYRING_SERVICE: &str = "logline-cli";
const KEYRING_AUTH_USER: &str = "auth_tokens";
const KEYRING_PASSKEY_USER: &str = "passkey_ed25519";

pub fn load_auth() -> Option<StoredAuth> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_AUTH_USER).ok()?;
    let json = entry.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

pub fn save_auth(auth: &StoredAuth) -> anyhow::Result<()> {
    let json = serde_json::to_string(auth)?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_AUTH_USER)
        .map_err(|e| anyhow::anyhow!("Keychain error: {e}"))?;
    entry
        .set_password(&json)
        .map_err(|e| anyhow::anyhow!("Failed to store auth in keychain: {e}"))?;

    // Clean up any legacy file-based auth
    let path = auth_path();
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

pub fn delete_auth() -> anyhow::Result<()> {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_AUTH_USER) {
        let _ = entry.delete_credential();
    }
    let path = auth_path();
    if path.exists() {
        fs::remove_file(path)?;
    }

    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_PASSKEY_USER) {
        let _ = entry.delete_credential();
    }
    let passkey_path = config_dir().join("passkey.json");
    if passkey_path.exists() {
        fs::remove_file(passkey_path)?;
    }
    Ok(())
}

pub fn load_passkey() -> Option<serde_json::Value> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_PASSKEY_USER).ok()?;
    let json = entry.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

pub fn save_passkey(data: &serde_json::Value) -> anyhow::Result<()> {
    let json = serde_json::to_string(data)?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_PASSKEY_USER)
        .map_err(|e| anyhow::anyhow!("Keychain error: {e}"))?;
    entry
        .set_password(&json)
        .map_err(|e| anyhow::anyhow!("Failed to store passkey in keychain: {e}"))?;

    // Clean up any legacy file-based passkey
    let path = config_dir().join("passkey.json");
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

// ─── Supabase HTTP client ───────────────────────────────────────────────────

pub struct SupabaseClient {
    pub config: SupabaseConfig,
    http: Client,
}

impl SupabaseClient {
    pub fn new(config: SupabaseConfig) -> anyhow::Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { config, http })
    }

    // ── Auth endpoints ──────────────────────────────────────────────────

    pub fn login_email(&self, email: &str, password: &str) -> anyhow::Result<AuthTokenResponse> {
        let url = format!(
            "{}/auth/v1/token?grant_type=password",
            self.config.url
        );
        let resp = self
            .http
            .post(&url)
            .header("apikey", &self.config.anon_key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()?;

        if resp.status().is_success() {
            Ok(resp.json()?)
        } else {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            bail!("Login failed ({status}): {body}")
        }
    }

    pub fn refresh_token(&self, refresh_token: &str) -> anyhow::Result<AuthTokenResponse> {
        let url = format!(
            "{}/auth/v1/token?grant_type=refresh_token",
            self.config.url
        );
        let resp = self
            .http
            .post(&url)
            .header("apikey", &self.config.anon_key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "refresh_token": refresh_token }))
            .send()?;

        if resp.status().is_success() {
            Ok(resp.json()?)
        } else {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            bail!("Token refresh failed ({status}): {body}")
        }
    }

    pub fn get_user(&self, access_token: &str) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/auth/v1/user", self.config.url);
        let resp = self
            .http
            .get(&url)
            .header("apikey", &self.config.anon_key)
            .header("Authorization", format!("Bearer {access_token}"))
            .send()?;

        if resp.status().is_success() {
            Ok(resp.json()?)
        } else {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            bail!("Get user failed ({status}): {body}")
        }
    }

    // ── PostgREST endpoints ─────────────────────────────────────────────

    pub fn postgrest_get(
        &self,
        table: &str,
        query: &str,
        access_token: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/rest/v1/{}?{}", self.config.url, table, query);
        let resp = self.postgrest_request("GET", &url, access_token, None)?;
        Ok(resp.json()?)
    }

    pub fn postgrest_insert(
        &self,
        table: &str,
        body: &serde_json::Value,
        access_token: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/rest/v1/{}", self.config.url, table);
        let resp = self.postgrest_request("POST", &url, access_token, Some(body))?;
        Ok(resp.json().unwrap_or(serde_json::json!({"ok": true})))
    }

    pub fn postgrest_upsert(
        &self,
        table: &str,
        body: &serde_json::Value,
        on_conflict: &str,
        access_token: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!(
            "{}/rest/v1/{}?on_conflict={}",
            self.config.url, table, on_conflict
        );
        let resp = self
            .http
            .post(&url)
            .header("apikey", &self.config.anon_key)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates,return=representation")
            .json(body)
            .send()?;

        if resp.status().is_success() {
            Ok(resp.json().unwrap_or(serde_json::json!({"ok": true})))
        } else {
            let status = resp.status();
            let body_text = resp.text().unwrap_or_default();
            bail!("PostgREST upsert {table} failed ({status}): {body_text}")
        }
    }

    fn postgrest_request(
        &self,
        method: &str,
        url: &str,
        access_token: &str,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<Response> {
        let mut req = match method {
            "POST" => self.http.post(url),
            "PATCH" => self.http.patch(url),
            "DELETE" => self.http.delete(url),
            _ => self.http.get(url),
        };

        req = req
            .header("apikey", &self.config.anon_key)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Prefer", "return=representation");

        if let Some(b) = body {
            req = req.header("Content-Type", "application/json").json(b);
        }

        let resp = req.send()?;

        if resp.status().is_success() {
            Ok(resp)
        } else {
            let status = resp.status();
            let body_text = resp.text().unwrap_or_default();
            bail!("PostgREST request failed ({status}): {body_text}")
        }
    }

    // ── Service-role operations (bootstrap only) ────────────────────────

    pub fn service_role_insert(
        &self,
        table: &str,
        body: &serde_json::Value,
        service_role_key: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/rest/v1/{}", self.config.url, table);
        let resp = self
            .http
            .post(&url)
            .header("apikey", &self.config.anon_key)
            .header("Authorization", format!("Bearer {service_role_key}"))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates,return=representation")
            .json(body)
            .send()?;

        if resp.status().is_success() {
            Ok(resp.json().unwrap_or(serde_json::json!({"ok": true})))
        } else {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            bail!("Service-role insert into {table} failed ({status}): {text}")
        }
    }
}

// ─── Auth token response ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    #[allow(dead_code)]
    pub token_type: String,
    pub user: AuthUser,
}

#[derive(Debug, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
}

// ─── Token management with auto-refresh ─────────────────────────────────────

pub fn get_valid_token(client: &SupabaseClient) -> anyhow::Result<String> {
    let auth = load_auth().ok_or_else(|| {
        anyhow::anyhow!(
            "Not logged in.\nRun `logline auth login --email <email>` first."
        )
    })?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Some(exp) = auth.expires_at {
        if now < exp.saturating_sub(30) {
            return Ok(auth.access_token);
        }
    }

    eprintln!("Token expired, refreshing...");
    match client.refresh_token(&auth.refresh_token) {
        Ok(fresh) => {
            let new_auth = StoredAuth {
                access_token: fresh.access_token.clone(),
                refresh_token: fresh.refresh_token,
                user_id: Some(fresh.user.id),
                email: fresh.user.email,
                expires_at: Some(now + fresh.expires_in),
                auth_method: auth.auth_method.clone(),
            };
            save_auth(&new_auth)?;
            Ok(fresh.access_token)
        }
        Err(e) => {
            bail!(
                "Session expired and refresh failed: {e}\n\
                 Run `logline auth login --email <email>` to re-authenticate."
            )
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn parse_env_value(content: &str, key: &str) -> Option<String> {
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, '=');
        let k = parts.next()?.trim();
        let v = parts.next()?.trim();
        if k != key {
            continue;
        }
        let unquoted = if (v.starts_with('"') && v.ends_with('"'))
            || (v.starts_with('\'') && v.ends_with('\''))
        {
            v[1..v.len().saturating_sub(1)].trim()
        } else {
            v
        };
        if !unquoted.is_empty() {
            return Some(unquoted.to_string());
        }
    }
    None
}
