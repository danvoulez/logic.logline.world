use anyhow::{bail, ensure};
use clap::Subcommand;

use crate::commands::secrets;

const SESSION_KEY: &str = "logline_session";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionToken {
    pub session_id: String,
    pub expires_at: u64,
    pub opened_by: String,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommands {
    /// Unlock session with Touch ID (required before any privileged command)
    Unlock {
        /// Session TTL (e.g. "5m", "30m", "2h"). Default: 30m
        #[arg(long, default_value = "30m")]
        ttl: String,
    },
    /// Lock session immediately (revoke access)
    Lock,
    /// Show session status and remaining TTL
    Status,
}

fn parse_ttl(ttl: &str) -> anyhow::Result<u64> {
    let s = ttl.trim().to_lowercase();
    if let Some(mins) = s.strip_suffix('m') {
        let n: u64 = mins.parse().map_err(|_| anyhow::anyhow!("Invalid TTL: {ttl}"))?;
        return Ok(n * 60);
    }
    if let Some(hours) = s.strip_suffix('h') {
        let n: u64 = hours.parse().map_err(|_| anyhow::anyhow!("Invalid TTL: {ttl}"))?;
        return Ok(n * 3600);
    }
    if let Some(secs) = s.strip_suffix('s') {
        let n: u64 = secs.parse().map_err(|_| anyhow::anyhow!("Invalid TTL: {ttl}"))?;
        return Ok(n);
    }
    bail!("Invalid TTL format: {ttl}. Use e.g. '5m', '30m', '2h'")
}

fn generate_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("s_{:x}", ts & 0xFFFF_FFFF)
}

fn touch_id_prompt() -> anyhow::Result<()> {
    if !cfg!(target_os = "macos") {
        eprint!("Press Enter to confirm identity: ");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        return Ok(());
    }

    eprintln!("Touch ID required...");
    let result = std::process::Command::new("swift")
        .arg("-e")
        .arg(
            r#"
import LocalAuthentication
import Foundation
let ctx = LAContext()
var err: NSError?
guard ctx.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &err) else {
    fputs("biometrics unavailable: \(err?.localizedDescription ?? "unknown")\n", stderr)
    exit(1)
}
let sema = DispatchSemaphore(value: 0)
var ok = false
ctx.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, localizedReason: "Logline CLI — unlock session") { success, _ in
    ok = success
    sema.signal()
}
sema.wait()
exit(ok ? 0 : 1)
"#,
        )
        .output();

    match result {
        Ok(out) if out.status.success() => Ok(()),
        Ok(_) => bail!("Touch ID authentication failed or was cancelled."),
        Err(e) => {
            eprintln!("Touch ID unavailable ({e}), falling back to Enter confirmation.");
            eprint!("Press Enter to confirm identity: ");
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf)?;
            Ok(())
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load_session() -> Option<SessionToken> {
    let json = secrets::load_credential(SESSION_KEY)?;
    serde_json::from_str(&json).ok()
}

fn save_session(token: &SessionToken) -> anyhow::Result<()> {
    let json = serde_json::to_string(token)?;
    secrets::store_credential(SESSION_KEY, &json)
}

fn delete_session() -> anyhow::Result<()> {
    let entry = keyring::Entry::new("logline-cli", SESSION_KEY)
        .map_err(|e| anyhow::anyhow!("Keychain error: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => bail!("Failed to clear session: {e}"),
    }
}

/// Gate: call at the top of every privileged command.
/// Returns the active session or a clear error.
pub fn require_unlocked() -> anyhow::Result<SessionToken> {
    let session = load_session().ok_or_else(|| {
        anyhow::anyhow!("Session locked. Run `logline auth unlock` first.")
    })?;
    ensure!(
        session.expires_at > now_secs(),
        "Session expired. Run `logline auth unlock` to re-authenticate."
    );
    Ok(session)
}

pub fn cmd_auth_session(command: SessionCommands, json: bool) -> anyhow::Result<()> {
    match command {
        SessionCommands::Unlock { ttl } => {
            let ttl_secs = parse_ttl(&ttl)?;
            touch_id_prompt()?;

            let session = SessionToken {
                session_id: generate_session_id(),
                expires_at: now_secs() + ttl_secs,
                opened_by: "touch_id".into(),
            };
            save_session(&session)?;

            let expires_str = format_expires(session.expires_at);
            crate::pout(
                json,
                serde_json::json!({
                    "ok": true,
                    "session_id": session.session_id,
                    "expires_at": session.expires_at,
                    "ttl_seconds": ttl_secs,
                }),
                &format!(
                    "Session active until {expires_str}. ID: {}",
                    session.session_id
                ),
            )
        }
        SessionCommands::Lock => {
            delete_session()?;
            crate::pout(
                json,
                serde_json::json!({"ok": true}),
                "Session locked. All privileged commands now require `logline auth unlock`.",
            )
        }
        SessionCommands::Status => {
            match load_session() {
                Some(s) if s.expires_at > now_secs() => {
                    let remaining = s.expires_at - now_secs();
                    let mins = remaining / 60;
                    let secs = remaining % 60;
                    crate::pout(
                        json,
                        serde_json::json!({
                            "unlocked": true,
                            "session_id": s.session_id,
                            "expires_at": s.expires_at,
                            "remaining_seconds": remaining,
                        }),
                        &format!(
                            "Unlocked — {mins}m {secs}s remaining. ID: {}",
                            s.session_id
                        ),
                    )
                }
                Some(_) => {
                    delete_session()?;
                    crate::pout(
                        json,
                        serde_json::json!({"unlocked": false, "reason": "expired"}),
                        "Session expired. Run `logline auth unlock` to re-authenticate.",
                    )
                }
                None => crate::pout(
                    json,
                    serde_json::json!({"unlocked": false, "reason": "no_session"}),
                    "No active session. Run `logline auth unlock` first.",
                ),
            }
        }
    }
}

fn format_expires(epoch: u64) -> String {
    let now = now_secs();
    if epoch <= now {
        return "expired".into();
    }
    let remaining = epoch - now;
    let hours = remaining / 3600;
    let mins = (remaining % 3600) / 60;
    format!("{hours}h {mins}m from now")
}

// ═══════════════════════════════════════════════════════════════════════════
// Auth Identity — WHO is logged in, HOW, and WHAT capabilities they have
// ═══════════════════════════════════════════════════════════════════════════

const IDENTITY_CACHE_KEY: &str = "logline_identity_cache";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthIdentity {
    pub user_id: String,
    pub email: Option<String>,
    pub auth_method: String,
    pub is_founder: bool,
    pub profile: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct IdentityCache {
    user_id: String,
    is_founder: bool,
    cached_at: u64,
}

impl IdentityCache {
    fn is_valid(&self) -> bool {
        now_secs().saturating_sub(self.cached_at) < 1800
    }
}

fn load_identity_cache() -> Option<IdentityCache> {
    let json = secrets::load_credential(IDENTITY_CACHE_KEY)?;
    serde_json::from_str(&json).ok()
}

fn save_identity_cache(cache: &IdentityCache) -> anyhow::Result<()> {
    let json = serde_json::to_string(cache)?;
    secrets::store_credential(IDENTITY_CACHE_KEY, &json)
}

fn check_founder_remote(user_id: &str) -> anyhow::Result<bool> {
    let supabase_url = secrets::load_credential_or_env("supabase_url", "NEXT_PUBLIC_SUPABASE_URL")
        .ok_or_else(|| anyhow::anyhow!("supabase_url not configured"))?;
    let anon_key = secrets::load_credential_or_env("supabase_anon_key", "NEXT_PUBLIC_SUPABASE_ANON_KEY")
        .ok_or_else(|| anyhow::anyhow!("supabase_anon_key not configured"))?;

    let auth = crate::supabase::load_auth()
        .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

    let url = format!(
        "{}/rest/v1/user_capabilities?select=capability&user_id=eq.{}&capability=eq.founder",
        supabase_url, user_id
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp = client
        .get(&url)
        .header("apikey", &anon_key)
        .header("Authorization", format!("Bearer {}", auth.access_token))
        .send()?;

    if !resp.status().is_success() {
        bail!("PostgREST query failed: {}", resp.status());
    }

    let body: serde_json::Value = resp.json()?;
    let is_founder = body
        .as_array()
        .is_some_and(|arr| !arr.is_empty());

    Ok(is_founder)
}

fn resolve_founder_status(user_id: &str) -> bool {
    if let Some(cache) = load_identity_cache() {
        if cache.user_id == user_id && cache.is_valid() {
            return cache.is_founder;
        }
    }

    match check_founder_remote(user_id) {
        Ok(is_founder) => {
            let _ = save_identity_cache(&IdentityCache {
                user_id: user_id.to_string(),
                is_founder,
                cached_at: now_secs(),
            });
            is_founder
        }
        Err(_) => {
            if let Some(cache) = load_identity_cache() {
                if cache.user_id == user_id {
                    return cache.is_founder;
                }
            }
            true
        }
    }
}

pub fn load_identity() -> Option<AuthIdentity> {
    let auth = crate::supabase::load_auth()?;
    let user_id = auth.user_id.clone()?;
    let method = auth.auth_method.unwrap_or_else(|| "unknown".into());
    let is_founder = resolve_founder_status(&user_id);
    let profile = if is_founder { "founder" } else { "operator" };

    Some(AuthIdentity {
        user_id,
        email: auth.email,
        auth_method: method,
        is_founder,
        profile: profile.into(),
    })
}

pub fn require_logged_in() -> anyhow::Result<AuthIdentity> {
    load_identity().ok_or_else(|| {
        anyhow::anyhow!(
            "Not logged in.\n\
             Run: logline auth login --passkey"
        )
    })
}

pub fn require_passkey_identity() -> anyhow::Result<AuthIdentity> {
    let identity = require_logged_in()?;
    ensure!(
        identity.auth_method == "passkey",
        "Infra commands require passkey authentication.\n\
         Current method: {}\n\
         Fix: logline auth login --passkey",
        identity.auth_method
    );
    Ok(identity)
}

pub fn require_non_founder(identity: &AuthIdentity) -> anyhow::Result<()> {
    ensure!(
        !identity.is_founder,
        "Infra commands cannot run as founder/god mode.\n\
         Current identity: {} ({})\n\
         Founder mode is reserved for `logline founder bootstrap` only.\n\
         Fix: log in as your operator/service user, not the founder account.",
        identity.email.as_deref().unwrap_or("?"),
        identity.user_id
    );
    Ok(())
}

/// Single uber-gate for all infra commands (deploy, cicd, db migrate).
/// Chains: require_unlocked + require_passkey_identity + require_non_founder.
pub fn require_infra_identity() -> anyhow::Result<(SessionToken, AuthIdentity)> {
    let session = require_unlocked()?;
    let identity = require_passkey_identity()?;
    require_non_founder(&identity)?;
    Ok((session, identity))
}
