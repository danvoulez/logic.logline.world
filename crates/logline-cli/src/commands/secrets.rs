use anyhow::{bail, ensure};
use clap::Subcommand;

const KEYRING_SERVICE: &str = "logline-cli";

const ALL_KEYS: &[&str] = &[
    "supabase_url",
    "supabase_anon_key",
    "supabase_service_role_key",
    "supabase_access_token",
    "database_url",
    "database_url_unpooled",
    "github_token",
    "vercel_token",
    "vercel_org_id",
    "vercel_project_id",
];

#[derive(Debug, Subcommand)]
pub enum SecretsCommands {
    /// Store a credential in macOS Keychain (prompted, never echoed)
    Set {
        /// Credential key (e.g. github_token, database_url)
        key: String,
    },
    /// Retrieve a credential from Keychain (requires unlocked session)
    Get {
        /// Credential key
        key: String,
    },
    /// List all stored credential keys (names only, never values)
    Ls,
    /// Remove a credential from Keychain
    Rm {
        /// Credential key to remove
        key: String,
    },
    /// Remove ALL stored credentials
    Clear,
    /// Check vault completeness against pipeline requirements
    Doctor,
}

pub fn store_credential(key: &str, value: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, key)
        .map_err(|e| anyhow::anyhow!("Keychain error: {e}"))?;
    entry
        .set_password(value)
        .map_err(|e| anyhow::anyhow!("Failed to store '{key}' in keychain: {e}"))?;
    Ok(())
}

pub fn load_credential(key: &str) -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, key).ok()?;
    entry.get_password().ok()
}

pub fn load_credential_or_env(keychain_key: &str, env_var: &str) -> Option<String> {
    if let Some(val) = load_credential(keychain_key) {
        return Some(val);
    }
    std::env::var(env_var).ok().filter(|v| !v.is_empty())
}

pub fn require_credential(key: &str) -> anyhow::Result<String> {
    load_credential(key).ok_or_else(|| {
        anyhow::anyhow!(
            "Credential '{key}' not found in keychain.\n\
             Store it with: logline secrets set {key}"
        )
    })
}

pub fn require_credential_or_env(keychain_key: &str, env_var: &str) -> anyhow::Result<String> {
    load_credential_or_env(keychain_key, env_var).ok_or_else(|| {
        anyhow::anyhow!(
            "Credential '{keychain_key}' not found.\n\
             Store it with: logline secrets set {keychain_key}\n\
             Or set env var: {env_var}"
        )
    })
}

fn delete_credential(key: &str) -> anyhow::Result<bool> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, key)
        .map_err(|e| anyhow::anyhow!("Keychain error: {e}"))?;
    match entry.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => bail!("Failed to delete '{key}': {e}"),
    }
}

fn validate_key(key: &str) -> anyhow::Result<()> {
    ensure!(
        !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "Invalid key '{key}'. Use lowercase alphanumeric + underscores (e.g. github_token)"
    );
    Ok(())
}

pub fn cmd_secrets(command: SecretsCommands, json: bool) -> anyhow::Result<()> {
    match command {
        SecretsCommands::Set { key } => {
            validate_key(&key)?;
            let value = rpassword::prompt_password(format!("Enter value for '{key}' (hidden): "))?;
            ensure!(!value.trim().is_empty(), "Value cannot be empty");
            store_credential(&key, value.trim())?;
            crate::pout(
                json,
                serde_json::json!({"ok": true, "key": key}),
                &format!("Stored: {key} -> Keychain"),
            )
        }
        SecretsCommands::Get { key } => {
            crate::require_unlocked()?;
            let value = require_credential(&key)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"key": key, "value": value}))?
                );
            } else {
                print!("{value}");
            }
            Ok(())
        }
        SecretsCommands::Ls => {
            let mut entries = Vec::new();
            for &key in ALL_KEYS {
                let present = load_credential(key).is_some();
                entries.push(serde_json::json!({"key": key, "stored": present}));
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for entry in &entries {
                    let k = entry["key"].as_str().unwrap_or("?");
                    let stored = entry["stored"].as_bool().unwrap_or(false);
                    let mark = if stored { "✓" } else { "✗" };
                    println!("  {k:<30} {mark}");
                }
            }
            Ok(())
        }
        SecretsCommands::Rm { key } => {
            validate_key(&key)?;
            let deleted = delete_credential(&key)?;
            if deleted {
                crate::pout(
                    json,
                    serde_json::json!({"ok": true, "key": key}),
                    &format!("Removed: {key}"),
                )
            } else {
                crate::pout(
                    json,
                    serde_json::json!({"ok": false, "key": key, "reason": "not_found"}),
                    &format!("Key '{key}' was not stored"),
                )
            }
        }
        SecretsCommands::Clear => {
            let mut removed = 0u32;
            for &key in ALL_KEYS {
                if delete_credential(key)? {
                    removed += 1;
                }
            }
            crate::pout(
                json,
                serde_json::json!({"ok": true, "removed": removed}),
                &format!("Cleared {removed} credentials from keychain"),
            )
        }
        SecretsCommands::Doctor => {
            cmd_secrets_doctor(json)
        }
    }
}

const REQUIRED_FOR_DEPLOY: &[(&str, &str)] = &[
    ("database_url", "Supabase DB migrations"),
    ("github_token", "GitHub push / PR / release"),
    ("vercel_token", "Vercel deployment polling"),
    ("vercel_org_id", "Vercel API calls"),
    ("vercel_project_id", "Vercel API calls"),
];

const REQUIRED_FOR_DEV: &[(&str, &str)] = &[
    ("database_url", "npm run dev / drizzle"),
    ("database_url_unpooled", "drizzle-kit migrate/generate"),
];

const REQUIRED_FOR_AUTH: &[(&str, &str)] = &[
    ("supabase_url", "Supabase Auth login"),
    ("supabase_anon_key", "Supabase Auth login"),
];

const DANGEROUS_ENV_VARS: &[(&str, &str)] = &[
    ("DATABASE_URL", "database connection string"),
    ("DATABASE_URL_UNPOOLED", "database connection string"),
    ("SUPABASE_SERVICE_ROLE_KEY", "service role key (god mode)"),
    ("SUPABASE_ACCESS_TOKEN", "management API token"),
    ("GITHUB_TOKEN", "GitHub API token"),
    ("VERCEL_TOKEN", "Vercel API token"),
];

fn cmd_secrets_doctor(json: bool) -> anyhow::Result<()> {
    use crate::commands::auth_session;

    let groups: &[(&str, &[(&str, &str)])] = &[
        ("deploy", REQUIRED_FOR_DEPLOY),
        ("dev", REQUIRED_FOR_DEV),
        ("auth", REQUIRED_FOR_AUTH),
    ];

    let mut vault_ok = true;
    let mut report_groups = Vec::new();

    for &(group_name, keys) in groups {
        let mut missing: Vec<&str> = Vec::new();
        let mut present: Vec<&str> = Vec::new();

        for &(key, _purpose) in keys {
            if load_credential(key).is_some() {
                present.push(key);
            } else {
                missing.push(key);
                vault_ok = false;
            }
        }

        report_groups.push(serde_json::json!({
            "group": group_name,
            "present": present,
            "missing": missing,
        }));
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let session = auth_session::load_session();
    let session_ok = session.as_ref().is_some_and(|s| s.expires_at > now);
    let session_remaining = session.as_ref()
        .filter(|s| s.expires_at > now)
        .map(|s| s.expires_at - now);

    let identity = auth_session::load_identity();

    let logged_in = identity.is_some();
    let auth_method = identity.as_ref().map(|i| i.auth_method.as_str()).unwrap_or("none");
    let passkey_ok = auth_method == "passkey";
    let is_founder = identity.as_ref().is_some_and(|i| i.is_founder);
    let profile = identity.as_ref().map(|i| i.profile.as_str()).unwrap_or("none");
    let subject_email = identity.as_ref().and_then(|i| i.email.as_deref()).unwrap_or("?");
    let subject_id = identity.as_ref().map(|i| i.user_id.as_str()).unwrap_or("?");

    // Check for secrets leaking via environment variables
    let mut env_leaks: Vec<&str> = Vec::new();
    for &(var, _desc) in DANGEROUS_ENV_VARS {
        if std::env::var(var).ok().is_some_and(|v| !v.is_empty()) {
            env_leaks.push(var);
        }
    }
    let no_leaks = env_leaks.is_empty();

    let ready_for_infra = vault_ok && session_ok && logged_in && passkey_ok && !is_founder && no_leaks;

    let auth_report = serde_json::json!({
        "logged_in": logged_in,
        "method": auth_method,
        "subject": subject_id,
        "email": subject_email,
        "profile": profile,
        "is_founder": is_founder,
        "passkey_ok": passkey_ok,
        "founder_blocked": is_founder,
    });

    let report = serde_json::json!({
        "ready_for_infra": ready_for_infra,
        "vault_ok": vault_ok,
        "session_active": session_ok,
        "auth": auth_report,
        "groups": report_groups,
        "env_leaks": env_leaks,
        "no_leaks": no_leaks,
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Secrets Doctor\n");

    // Vault section
    for group in &report_groups {
        let name = group["group"].as_str().unwrap_or("?");
        let missing = group["missing"].as_array();
        let present = group["present"].as_array();
        let ok = missing.is_some_and(|m| m.is_empty());
        let mark = if ok { "✓" } else { "✗" };
        println!("{mark} vault/{name}:");

        if let Some(arr) = present {
            for k in arr {
                println!("    ✓ {}", k.as_str().unwrap_or("?"));
            }
        }
        if let Some(arr) = missing {
            for k in arr {
                let key = k.as_str().unwrap_or("?");
                println!("    ✗ {key}  <-- logline secrets set {key}");
            }
        }
    }

    // Session section
    println!();
    if session_ok {
        let rem = session_remaining.unwrap_or(0);
        let mins = rem / 60;
        let secs = rem % 60;
        println!("✓ session: active ({mins}m {secs}s remaining)");
    } else {
        println!("✗ session: locked  <-- logline auth unlock");
    }

    // Auth identity section
    println!();
    if !logged_in {
        println!("✗ auth:");
        println!("    logged_in: false");
        println!("    Fix: logline auth login --passkey");
    } else {
        let auth_mark = if passkey_ok && !is_founder { "✓" } else { "✗" };
        println!("{auth_mark} auth:");
        println!("    logged_in: true");

        let method_mark = if passkey_ok { "✓" } else { "✗" };
        println!("    {method_mark} method: {auth_method}{}",
            if !passkey_ok { "  <-- FAIL: must be passkey. Run: logline auth login --passkey" } else { "" }
        );

        println!("    subject: {subject_email} ({subject_id})");

        let profile_mark = if !is_founder { "✓" } else { "✗" };
        println!("    {profile_mark} profile: {profile}{}",
            if is_founder { "  <-- FAIL: founder cannot run infra. Use operator/service account." } else { "" }
        );
    }

    // Env leak section
    println!();
    if env_leaks.is_empty() {
        println!("✓ env: no secrets leaked via environment variables");
    } else {
        println!("✗ env: SECRETS FOUND IN ENVIRONMENT (bypass risk!)");
        for var in &env_leaks {
            println!("    ✗ {var}  <-- unset this or remove from .env files");
        }
    }

    // Summary
    println!();
    if ready_for_infra {
        println!("ready_for_infra: true");
        println!("\nAll systems go. Ready for `logline cicd run`.");
    } else {
        println!("ready_for_infra: false");
        println!();
        if !vault_ok { println!("  Fix: logline secrets set <key> (see vault above)"); }
        if !session_ok { println!("  Fix: logline auth unlock"); }
        if !logged_in { println!("  Fix: logline auth login --passkey"); }
        else if !passkey_ok { println!("  Fix: logline auth login --passkey"); }
        if is_founder { println!("  Fix: log in as operator/service user, not founder"); }
        if !no_leaks { println!("  Fix: remove secrets from environment variables (see env section above)"); }
    }

    Ok(())
}
