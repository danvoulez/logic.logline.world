mod commands;
mod integrations;
mod supabase;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use clap::{Parser, Subcommand};
use logline_api::{Intent, RuntimeEngine};
use logline_core::{
    default_config_dir, demo_catalog, load_catalog_from_dir, write_default_config_files,
};
use logline_runtime::LoglineRuntime;

use crate::commands::auth_session;
use crate::commands::cicd;
use crate::commands::db;
use crate::commands::deploy;
use crate::commands::dev;
use crate::commands::secrets;
use crate::supabase::{
    SupabaseClient, SupabaseConfig, StoredAuth,
    get_valid_token, load_auth, save_auth, delete_auth,
    load_passkey, save_passkey,
};

#[derive(Debug, Parser)]
#[command(name = "logline", about = "Logline CLI — one binary, Supabase direct")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,

    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init {
        #[arg(long)]
        force: bool,
    },
    Status,
    Run {
        #[arg(long)]
        intent: String,
        #[arg(long = "arg", value_parser = parse_key_val)]
        args: Vec<(String, String)>,
    },
    Stop { run_id: String },
    Events {
        #[arg(long)]
        since: Option<String>,
    },
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
    Backend {
        #[command(subcommand)]
        command: BackendCommands,
    },
    /// Authentication
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Founder operations (bootstrap, signing)
    Founder {
        #[command(subcommand)]
        command: FounderCommands,
    },
    /// App management (create, handshake, config)
    App {
        #[command(subcommand)]
        command: AppCommands,
    },
    /// Tenant management
    Tenant {
        #[command(subcommand)]
        command: TenantCommands,
    },
    /// Fuel ledger
    Fuel {
        #[command(subcommand)]
        command: FuelCommands,
    },
    /// Supabase CLI helper commands
    Supabase {
        #[command(subcommand)]
        command: SupabaseCommands,
    },
    /// Credential vault — store/retrieve secrets in macOS Keychain
    Secrets {
        #[command(subcommand)]
        command: secrets::SecretsCommands,
    },
    /// Database operations (query, tables, migrations, RLS verification)
    Db {
        #[command(subcommand)]
        command: db::DbCommands,
    },
    /// Development commands (build, start, migrate with injected credentials)
    Dev {
        #[command(subcommand)]
        command: dev::DevCommands,
    },
    /// Deploy to production (supabase, github, vercel, or all)
    Deploy {
        #[command(subcommand)]
        command: deploy::DeployCommands,
    },
    /// CI/CD pipeline runner (reads logline.cicd.json)
    Cicd {
        #[command(subcommand)]
        command: cicd::CicdCommands,
    },
    /// Pre-flight check: vault + session + identity + pipeline readiness
    Ready {
        /// Pipeline to check readiness for
        #[arg(long, default_value = "prod")]
        pipeline: String,
    },
}

#[derive(Debug, Subcommand)]
enum ProfileCommands {
    List,
    Use { profile_id: String },
}

#[derive(Debug, Subcommand)]
enum BackendCommands {
    List,
    Test { backend_id: String },
}

#[derive(Debug, Subcommand)]
enum AuthCommands {
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
    /// Login with email/password (Supabase Auth direct)
    Login {
        /// Email address
        #[arg(long)]
        email: Option<String>,
        /// Use passkey (Touch ID) to unlock stored refresh token
        #[arg(long)]
        passkey: bool,
    },
    /// Register a passkey (Ed25519 keypair + Touch ID gate)
    PasskeyRegister {
        /// Device name for this passkey
        #[arg(long)]
        device_name: Option<String>,
    },
    /// Show current identity
    Whoami,
    /// Remove stored tokens and logout
    Logout,
}

#[derive(Debug, Subcommand)]
enum FounderCommands {
    /// One-time world bootstrap (creates tenant, user, memberships, founder cap)
    Bootstrap {
        /// Tenant slug for the HQ tenant
        #[arg(long)]
        tenant_slug: String,
        /// Tenant display name
        #[arg(long)]
        tenant_name: String,
    },
}

#[derive(Debug, Subcommand)]
enum AppCommands {
    /// Register a new app under the current tenant
    Create {
        #[arg(long)]
        app_id: String,
        #[arg(long)]
        name: String,
    },
    /// Bidirectional handshake: store the app's service URL and API key
    Handshake {
        #[arg(long)]
        app_id: String,
        #[arg(long)]
        service_url: String,
        #[arg(long)]
        api_key: Option<String>,
        /// Comma-separated capabilities
        #[arg(long)]
        capabilities: Option<String>,
    },
    /// Export ecosystem config JSON for an app to consume
    ConfigExport {
        #[arg(long)]
        app_id: String,
    },
    /// List apps in the current tenant
    List,
}

#[derive(Debug, Subcommand)]
enum TenantCommands {
    /// Create a new tenant (founder only)
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
    },
    /// Add an email to the tenant allowlist
    AllowlistAdd {
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "member")]
        role: String,
        /// Comma-separated app:role pairs (e.g. "ublx:member,llm-gateway:member")
        #[arg(long)]
        app_defaults: Option<String>,
    },
    /// Resolve tenant by slug
    Resolve {
        #[arg(long)]
        slug: String,
    },
}

#[derive(Debug, Subcommand)]
enum FuelCommands {
    /// Emit a fuel event
    Emit {
        #[arg(long)]
        app_id: String,
        #[arg(long)]
        units: f64,
        #[arg(long)]
        unit_type: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SupabaseCommands {
    /// Store Supabase access token in OS keychain (never on disk)
    StoreToken,
    Check {
        #[arg(long)]
        workdir: Option<PathBuf>,
    },
    Projects {
        #[arg(long)]
        workdir: Option<PathBuf>,
    },
    Link {
        #[arg(long)]
        project_ref: String,
        #[arg(long)]
        workdir: Option<PathBuf>,
    },
    Migrate {
        #[arg(long)]
        workdir: Option<PathBuf>,
    },
    Raw {
        #[arg(long)]
        workdir: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg_dir = cli.config_dir.clone().unwrap_or_else(default_config_dir);

    let catalog = match load_catalog_from_dir(&cfg_dir) {
        Ok(c) => c,
        Err(_) => demo_catalog(),
    };
    let runtime = LoglineRuntime::from_catalog(catalog.clone())?;

    match cli.command {
        Commands::Init { force } => {
            if force && cfg_dir.exists() {
                for name in ["connections.toml", "runtime.toml", "ui.toml"] {
                    let p = cfg_dir.join(name);
                    if p.exists() {
                        fs::remove_file(&p)?;
                    }
                }
            }
            write_default_config_files(&cfg_dir)?;
            pout(cli.json, serde_json::json!({"message":"init complete","config_dir":cfg_dir}), "Init complete")?;
        }
        Commands::Status => {
            let status = runtime.status()?;
            pout(cli.json, serde_json::to_value(status)?, "Runtime status retrieved")?;
        }
        Commands::Run { intent, args } => {
            let payload = BTreeMap::from_iter(args);
            let result = runtime.run_intent(Intent { intent_type: intent, payload })?;
            pout(cli.json, serde_json::to_value(result)?, "Intent accepted")?;
        }
        Commands::Stop { run_id } => {
            runtime.stop_run(run_id.clone())?;
            pout(cli.json, serde_json::json!({"ok":true,"run_id":run_id}), "Stop signal sent")?;
        }
        Commands::Events { since } => {
            let events = runtime.events_since(since)?;
            pout(cli.json, serde_json::to_value(events)?, "Events fetched")?;
        }
        Commands::Profile { command } => match command {
            ProfileCommands::List => {
                let profiles: Vec<_> = catalog.profiles.keys().cloned().collect();
                pout(cli.json, serde_json::to_value(profiles)?, "Profiles listed")?;
            }
            ProfileCommands::Use { profile_id } => {
                runtime.select_profile(profile_id.clone())?;
                pout(cli.json, serde_json::json!({"ok":true,"active_profile":profile_id}), "Profile selected")?;
            }
        },
        Commands::Backend { command } => match command {
            BackendCommands::List => {
                let backends: Vec<_> = catalog.backends.keys().cloned().collect();
                pout(cli.json, serde_json::to_value(backends)?, "Backends listed")?;
            }
            BackendCommands::Test { backend_id } => {
                runtime.test_backend(backend_id.clone())?;
                pout(cli.json, serde_json::json!({"ok":true,"backend_id":backend_id}), "Backend health check passed")?;
            }
        },

        // ─── Auth ───────────────────────────────────────────────────────
        Commands::Auth { command } => {
            match &command {
                AuthCommands::Unlock { ttl } => {
                    return auth_session::cmd_auth_session(
                        auth_session::SessionCommands::Unlock { ttl: ttl.clone() },
                        cli.json,
                    );
                }
                AuthCommands::Lock => {
                    return auth_session::cmd_auth_session(
                        auth_session::SessionCommands::Lock,
                        cli.json,
                    );
                }
                AuthCommands::Status => {
                    return auth_session::cmd_auth_session(
                        auth_session::SessionCommands::Status,
                        cli.json,
                    );
                }
                _ => {}
            }

            let config = SupabaseConfig::from_env_or_file()?;
            let client = SupabaseClient::new(config)?;

            match command {
                AuthCommands::Unlock { .. } | AuthCommands::Lock | AuthCommands::Status => unreachable!(),
                AuthCommands::Login { email, passkey } => {
                    if passkey {
                        cmd_login_passkey(&client, cli.json)?;
                    } else {
                        let email = email.ok_or_else(|| {
                            anyhow::anyhow!("--email <address> is required.\nUsage: logline auth login --email you@example.com")
                        })?;
                        cmd_login_email(&client, &email, cli.json)?;
                    }
                }
                AuthCommands::PasskeyRegister { device_name } => {
                    cmd_passkey_register(&client, device_name, cli.json)?;
                }
                AuthCommands::Whoami => {
                    cmd_whoami(&client, cli.json)?;
                }
                AuthCommands::Logout => {
                    delete_auth()?;
                    pout(cli.json, serde_json::json!({"ok":true}), "Logged out. All local tokens removed.")?;
                }
            }
        }

        // ─── Founder ────────────────────────────────────────────────────
        Commands::Founder { command } => {
            let config = SupabaseConfig::from_env_or_file()?;
            let client = SupabaseClient::new(config)?;

            match command {
                FounderCommands::Bootstrap { tenant_slug, tenant_name } => {
                    cmd_founder_bootstrap(&client, &tenant_slug, &tenant_name, cli.json)?;
                }
            }
        }

        // ─── App ────────────────────────────────────────────────────────
        Commands::App { command } => {
            let config = SupabaseConfig::from_env_or_file()?;
            let client = SupabaseClient::new(config)?;

            match command {
                AppCommands::Create { app_id, name } => {
                    cmd_app_create(&client, &app_id, &name, cli.json)?;
                }
                AppCommands::Handshake { app_id, service_url, api_key, capabilities } => {
                    cmd_app_handshake(&client, &app_id, &service_url, api_key.as_deref(), capabilities.as_deref(), cli.json)?;
                }
                AppCommands::ConfigExport { app_id } => {
                    cmd_app_config_export(&client, &app_id, cli.json)?;
                }
                AppCommands::List => {
                    cmd_app_list(&client, cli.json)?;
                }
            }
        }

        // ─── Tenant ─────────────────────────────────────────────────────
        Commands::Tenant { command } => {
            let config = SupabaseConfig::from_env_or_file()?;
            let client = SupabaseClient::new(config)?;

            match command {
                TenantCommands::Create { slug, name } => {
                    cmd_tenant_create(&client, &slug, &name, cli.json)?;
                }
                TenantCommands::AllowlistAdd { email, role, app_defaults } => {
                    cmd_tenant_allowlist_add(&client, &email, &role, app_defaults.as_deref(), cli.json)?;
                }
                TenantCommands::Resolve { slug } => {
                    cmd_tenant_resolve(&client, &slug, cli.json)?;
                }
            }
        }

        // ─── Fuel ───────────────────────────────────────────────────────
        Commands::Fuel { command } => {
            let config = SupabaseConfig::from_env_or_file()?;
            let client = SupabaseClient::new(config)?;

            match command {
                FuelCommands::Emit { app_id, units, unit_type, source, idempotency_key } => {
                    cmd_fuel_emit(&client, &app_id, units, &unit_type, &source, idempotency_key.as_deref(), cli.json)?;
                }
            }
        }

        // ─── New CLI-Only commands ──────────────────────────────────────
        Commands::Secrets { command } => {
            return secrets::cmd_secrets(command, cli.json);
        }
        Commands::Db { command } => {
            return db::cmd_db(command, cli.json);
        }
        Commands::Dev { command } => {
            return dev::cmd_dev(command, cli.json);
        }
        Commands::Deploy { command } => {
            return deploy::cmd_deploy(command, cli.json);
        }
        Commands::Cicd { command } => {
            return cicd::cmd_cicd(command, cli.json);
        }
        Commands::Ready { pipeline } => {
            return cmd_ready(&pipeline, cli.json);
        }

        // ─── Supabase CLI helpers (legacy) ──────────────────────────────
        Commands::Supabase { command } => match command {
            SupabaseCommands::StoreToken => {
                let token = rpassword::prompt_password("Supabase Access Token (paste, hidden): ")?;
                if token.trim().is_empty() {
                    anyhow::bail!("Token cannot be empty");
                }
                let entry = keyring::Entry::new("logline-cli", "supabase_access_token")
                    .map_err(|e| anyhow::anyhow!("Keychain error: {e}"))?;
                entry.set_password(token.trim())
                    .map_err(|e| anyhow::anyhow!("Failed to store in keychain: {e}"))?;
                pout(cli.json, serde_json::json!({"ok": true}), "Supabase access token stored in OS keychain.")?;
            }
            SupabaseCommands::Check { workdir } => {
                println!("supabase version:");
                run_supabase_stream(&["--version"], workdir.as_ref())?;
                println!("\nProjects:");
                run_supabase_stream(&["projects", "list"], workdir.as_ref())?;
            }
            SupabaseCommands::Projects { workdir } => {
                run_supabase_stream(&["projects", "list"], workdir.as_ref())?;
            }
            SupabaseCommands::Link { project_ref, workdir } => {
                run_supabase_stream(&["link", "--project-ref", &project_ref], workdir.as_ref())?;
            }
            SupabaseCommands::Migrate { workdir } => {
                run_supabase_stream(&["db", "push"], workdir.as_ref())?;
            }
            SupabaseCommands::Raw { workdir, args } => {
                let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                run_supabase_stream(&str_args, workdir.as_ref())?;
            }
        },
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Command implementations
// ═══════════════════════════════════════════════════════════════════════════

fn cmd_login_email(client: &SupabaseClient, email: &str, json: bool) -> anyhow::Result<()> {
    let password = rpassword::prompt_password(format!("Password for {email}: "))?;
    if password.is_empty() {
        anyhow::bail!("Password cannot be empty");
    }

    let resp = client.login_email(email, &password)?;
    let now = now_secs();

    let stored = StoredAuth {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        user_id: Some(resp.user.id.clone()),
        email: resp.user.email.clone(),
        expires_at: Some(now + resp.expires_in),
        auth_method: Some("password".into()),
    };
    save_auth(&stored)?;

    pout(json, serde_json::json!({
        "ok": true,
        "user_id": resp.user.id,
        "email": resp.user.email,
        "auth_method": "password",
    }), &format!("Logged in as {} ({})", resp.user.email.as_deref().unwrap_or("?"), resp.user.id))?;

    Ok(())
}

fn cmd_login_passkey(client: &SupabaseClient, json: bool) -> anyhow::Result<()> {
    let auth = load_auth().ok_or_else(|| {
        anyhow::anyhow!("No stored session. Run `logline auth login --email` first, then register a passkey.")
    })?;

    if load_passkey().is_none() {
        anyhow::bail!("No passkey registered. Run `logline auth passkey-register` first.");
    }

    // Touch ID gate (macOS)
    if cfg!(target_os = "macos") {
        eprintln!("Touch ID required to unlock session...");
        let result = std::process::Command::new("swift")
            .arg("-e")
            .arg(r#"
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
ctx.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, localizedReason: "Logline CLI authentication") { success, _ in
    ok = success
    sema.signal()
}
sema.wait()
exit(ok ? 0 : 1)
"#)
            .output();

        match result {
            Ok(out) if out.status.success() => {}
            Ok(_) => anyhow::bail!("Touch ID authentication failed or was cancelled."),
            Err(e) => {
                eprintln!("Touch ID unavailable ({e}), falling back to Enter confirmation.");
                eprint!("Press Enter to confirm identity: ");
                let mut buf = String::new();
                std::io::stdin().read_line(&mut buf)?;
            }
        }
    } else {
        eprint!("Press Enter to confirm identity: ");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
    }

    let resp = client.refresh_token(&auth.refresh_token)?;
    let now = now_secs();

    let stored = StoredAuth {
        access_token: resp.access_token.clone(),
        refresh_token: resp.refresh_token,
        user_id: Some(resp.user.id.clone()),
        email: resp.user.email.clone(),
        expires_at: Some(now + resp.expires_in),
        auth_method: Some("passkey".into()),
    };
    save_auth(&stored)?;

    pout(json, serde_json::json!({
        "ok": true,
        "user_id": resp.user.id,
        "email": resp.user.email,
        "auth_method": "passkey",
    }), &format!("Authenticated via passkey as {}", resp.user.email.as_deref().unwrap_or(&resp.user.id)))?;

    Ok(())
}

fn cmd_passkey_register(client: &SupabaseClient, device_name: Option<String>, json: bool) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;
    let user_id = user["id"].as_str().ok_or_else(|| anyhow::anyhow!("Cannot determine user_id"))?;

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    let signing_key = SigningKey::generate(&mut OsRng);
    let public_key = signing_key.verifying_key();
    let public_key_hex = hex::encode(public_key.as_bytes());
    let private_key_hex = hex::encode(signing_key.to_bytes());

    let device = device_name.unwrap_or_else(get_hostname);

    let passkey_data = serde_json::json!({
        "device_name": device,
        "private_key": private_key_hex,
        "public_key": public_key_hex,
        "algorithm": "ed25519",
    });

    save_passkey(&passkey_data)?;

    // Register public key in cli_passkey_credentials via PostgREST
    let cred = serde_json::json!({
        "user_id": user_id,
        "device_name": device,
        "public_key": public_key_hex,
        "algorithm": "ed25519",
        "status": "active",
    });

    client.postgrest_upsert("cli_passkey_credentials", &cred, "user_id,device_name", &token)?;

    pout(json, serde_json::json!({
        "ok": true,
        "device_name": device,
        "public_key": public_key_hex,
    }), &format!("Passkey registered for device '{}'\nPublic key: {}", device, public_key_hex))?;

    Ok(())
}

fn cmd_whoami(client: &SupabaseClient, json: bool) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&user)?);
    } else {
        let id = user["id"].as_str().unwrap_or("?");
        let email = user["email"].as_str().unwrap_or("?");
        println!("User ID: {id}");
        println!("Email:   {email}");
    }

    Ok(())
}

fn cmd_founder_bootstrap(
    client: &SupabaseClient,
    tenant_slug: &str,
    tenant_name: &str,
    json: bool,
) -> anyhow::Result<()> {
    let service_role_key = std::env::var("SUPABASE_SERVICE_ROLE_KEY")
        .map_err(|_| anyhow::anyhow!(
            "SUPABASE_SERVICE_ROLE_KEY env var required for bootstrap.\n\
             This is a one-time operation. The service role key is never needed again."
        ))?;

    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;
    let user_id = user["id"].as_str().ok_or_else(|| anyhow::anyhow!("Cannot determine user_id from JWT"))?;
    let email = user["email"].as_str().unwrap_or("");
    let display_name = user["user_metadata"]["display_name"].as_str().unwrap_or(email);

    eprintln!("Bootstrapping world as {email} ({user_id})...");

    let tenant_id = tenant_slug.to_string();

    // All inserts use service-role key to bypass RLS (nothing exists yet)
    client.service_role_insert("users", &serde_json::json!({
        "user_id": user_id,
        "email": email,
        "display_name": display_name,
    }), &service_role_key)?;
    eprintln!("  ✓ User record created");

    client.service_role_insert("tenants", &serde_json::json!({
        "tenant_id": tenant_id,
        "slug": tenant_slug,
        "name": tenant_name,
    }), &service_role_key)?;
    eprintln!("  ✓ Tenant '{tenant_slug}' created");

    client.service_role_insert("tenant_memberships", &serde_json::json!({
        "tenant_id": tenant_id,
        "user_id": user_id,
        "role": "admin",
    }), &service_role_key)?;
    eprintln!("  ✓ Tenant membership (admin)");

    client.service_role_insert("user_capabilities", &serde_json::json!({
        "user_id": user_id,
        "capability": "founder",
        "granted_by": user_id,
    }), &service_role_key)?;
    eprintln!("  ✓ Founder capability granted");

    client.service_role_insert("apps", &serde_json::json!({
        "app_id": "ublx",
        "tenant_id": tenant_id,
        "name": "UBLX Headquarters",
    }), &service_role_key)?;
    eprintln!("  ✓ HQ app 'ublx' created");

    client.service_role_insert("app_memberships", &serde_json::json!({
        "app_id": "ublx",
        "tenant_id": tenant_id,
        "user_id": user_id,
        "role": "app_admin",
    }), &service_role_key)?;
    eprintln!("  ✓ App membership (app_admin)");

    eprintln!();
    eprintln!("WARNING: Consider rotating SUPABASE_SERVICE_ROLE_KEY in the");
    eprintln!("  Supabase Dashboard -> Settings -> API -> Service Role Key.");
    eprintln!("  The key used for bootstrap should not be reused.");

    pout(json, serde_json::json!({
        "ok": true,
        "tenant_id": tenant_id,
        "user_id": user_id,
        "app_id": "ublx",
    }), &format!(
        "\nBootstrap complete.\n\
         Tenant: {tenant_slug} ({tenant_name})\n\
         Founder: {email}\n\
         HQ App: ublx\n\n\
         Service role key is no longer needed. All operations now use JWT + RLS."
    ))?;

    Ok(())
}

fn cmd_app_create(client: &SupabaseClient, app_id: &str, name: &str, json: bool) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;
    let user_id = user["id"].as_str().ok_or_else(|| anyhow::anyhow!("Cannot determine user_id"))?;

    // Get first tenant membership to determine tenant_id
    let memberships = client.postgrest_get("tenant_memberships", &format!("select=tenant_id,role&user_id=eq.{user_id}&limit=1"), &token)?;
    let tenant_id = memberships.as_array()
        .and_then(|a| a.first())
        .and_then(|m| m["tenant_id"].as_str())
        .ok_or_else(|| anyhow::anyhow!("No tenant membership found. Run `logline founder bootstrap` first."))?;

    client.postgrest_insert("apps", &serde_json::json!({
        "app_id": app_id,
        "tenant_id": tenant_id,
        "name": name,
    }), &token)?;

    client.postgrest_insert("app_memberships", &serde_json::json!({
        "app_id": app_id,
        "tenant_id": tenant_id,
        "user_id": user_id,
        "role": "app_admin",
    }), &token)?;

    pout(json, serde_json::json!({
        "ok": true,
        "app_id": app_id,
        "tenant_id": tenant_id,
    }), &format!("App '{name}' ({app_id}) created under tenant {tenant_id}"))?;

    Ok(())
}

fn cmd_app_handshake(
    client: &SupabaseClient,
    app_id: &str,
    service_url: &str,
    api_key: Option<&str>,
    capabilities: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;
    let user_id = user["id"].as_str().unwrap_or("?");

    let memberships = client.postgrest_get("tenant_memberships", &format!("select=tenant_id&user_id=eq.{user_id}&limit=1"), &token)?;
    let tenant_id = memberships.as_array()
        .and_then(|a| a.first())
        .and_then(|m| m["tenant_id"].as_str())
        .ok_or_else(|| anyhow::anyhow!("No tenant membership found"))?;

    let caps: Vec<String> = capabilities
        .map(|c| c.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let body = serde_json::json!({
        "app_id": app_id,
        "tenant_id": tenant_id,
        "service_url": service_url,
        "api_key_encrypted": api_key.unwrap_or(""),
        "capabilities": caps,
        "status": "active",
        "onboarded_at": chrono_now(),
        "onboarded_by": user_id,
    });

    client.postgrest_upsert("app_service_config", &body, "app_id,tenant_id", &token)?;

    pout(json, serde_json::json!({
        "ok": true,
        "app_id": app_id,
        "service_url": service_url,
        "capabilities": caps,
    }), &format!("Handshake complete for '{app_id}'.\nHQ can now reach {service_url}"))?;

    Ok(())
}

fn cmd_app_config_export(client: &SupabaseClient, app_id: &str, json: bool) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;
    let user_id = user["id"].as_str().unwrap_or("?");

    let memberships = client.postgrest_get("tenant_memberships", &format!("select=tenant_id&user_id=eq.{user_id}&limit=1"), &token)?;
    let tenant_id = memberships.as_array()
        .and_then(|a| a.first())
        .and_then(|m| m["tenant_id"].as_str())
        .unwrap_or("?");

    let config = serde_json::json!({
        "supabase_url": client.config.url,
        "supabase_anon_key": client.config.anon_key,
        "app_id": app_id,
        "tenant_id": tenant_id,
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&config)?);
    } else {
        println!("Ecosystem config for '{app_id}':\n");
        println!("{}", serde_json::to_string_pretty(&config)?);
        println!("\nPaste this into the app's configuration.");
    }

    Ok(())
}

fn cmd_app_list(client: &SupabaseClient, json: bool) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let apps = client.postgrest_get("apps", "select=app_id,tenant_id,name,created_at", &token)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&apps)?);
    } else if let Some(arr) = apps.as_array() {
        if arr.is_empty() {
            println!("No apps found.");
        } else {
            for app in arr {
                println!("  {} — {} (tenant: {})",
                    app["app_id"].as_str().unwrap_or("?"),
                    app["name"].as_str().unwrap_or("?"),
                    app["tenant_id"].as_str().unwrap_or("?"),
                );
            }
        }
    }

    Ok(())
}

fn cmd_tenant_create(client: &SupabaseClient, slug: &str, name: &str, json: bool) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;

    client.postgrest_insert("tenants", &serde_json::json!({
        "tenant_id": slug,
        "slug": slug,
        "name": name,
    }), &token)?;

    pout(json, serde_json::json!({
        "ok": true,
        "tenant_id": slug,
        "slug": slug,
        "name": name,
    }), &format!("Tenant '{name}' ({slug}) created"))?;

    Ok(())
}

fn cmd_tenant_allowlist_add(
    client: &SupabaseClient,
    email: &str,
    role: &str,
    app_defaults: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;
    let user_id = user["id"].as_str().unwrap_or("?");

    let memberships = client.postgrest_get("tenant_memberships", &format!("select=tenant_id&user_id=eq.{user_id}&limit=1"), &token)?;
    let tenant_id = memberships.as_array()
        .and_then(|a| a.first())
        .and_then(|m| m["tenant_id"].as_str())
        .ok_or_else(|| anyhow::anyhow!("No tenant membership found"))?;

    let defaults: Vec<serde_json::Value> = app_defaults
        .map(|ad| {
            ad.split(',')
                .filter_map(|pair| {
                    let mut parts = pair.trim().splitn(2, ':');
                    let app = parts.next()?;
                    let r = parts.next().unwrap_or("member");
                    Some(serde_json::json!({"app_id": app, "role": r}))
                })
                .collect()
        })
        .unwrap_or_default();

    let email_norm = email.trim().to_lowercase();

    client.postgrest_upsert("tenant_email_allowlist", &serde_json::json!({
        "tenant_id": tenant_id,
        "email_normalized": email_norm,
        "role_default": role,
        "app_defaults": defaults,
    }), "tenant_id,email_normalized", &token)?;

    pout(json, serde_json::json!({
        "ok": true,
        "email": email_norm,
        "tenant_id": tenant_id,
        "role": role,
        "app_defaults": defaults,
    }), &format!("Added {email_norm} to allowlist (role: {role})"))?;

    Ok(())
}

fn cmd_tenant_resolve(client: &SupabaseClient, slug: &str, json: bool) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let tenants = client.postgrest_get("tenants", &format!("select=tenant_id,slug,name,created_at&slug=eq.{slug}"), &token)?;

    let tenant = tenants.as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow::anyhow!("Tenant with slug '{slug}' not found"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(tenant)?);
    } else {
        println!("Tenant: {} ({})", tenant["name"].as_str().unwrap_or("?"), tenant["tenant_id"].as_str().unwrap_or("?"));
    }

    Ok(())
}

fn cmd_fuel_emit(
    client: &SupabaseClient,
    app_id: &str,
    units: f64,
    unit_type: &str,
    source: &str,
    idempotency_key: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let token = get_valid_token(client)?;
    let user = client.get_user(&token)?;
    let user_id = user["id"].as_str().ok_or_else(|| anyhow::anyhow!("Cannot determine user_id"))?;

    let memberships = client.postgrest_get("tenant_memberships", &format!("select=tenant_id&user_id=eq.{user_id}&limit=1"), &token)?;
    let tenant_id = memberships.as_array()
        .and_then(|a| a.first())
        .and_then(|m| m["tenant_id"].as_str())
        .ok_or_else(|| anyhow::anyhow!("No tenant membership found"))?;

    let idem_key = idempotency_key
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}-{}-{}-{}", app_id, user_id, unit_type, now_secs()));

    client.postgrest_insert("fuel_events", &serde_json::json!({
        "idempotency_key": idem_key,
        "tenant_id": tenant_id,
        "app_id": app_id,
        "user_id": user_id,
        "units": units,
        "unit_type": unit_type,
        "source": source,
    }), &token)?;

    pout(json, serde_json::json!({
        "ok": true,
        "idempotency_key": idem_key,
        "app_id": app_id,
        "units": units,
        "unit_type": unit_type,
    }), &format!("Fuel event emitted: {units} {unit_type} for {app_id}"))?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Ready (pre-flight)
// ═══════════════════════════════════════════════════════════════════════════

fn cmd_ready(pipeline: &str, json: bool) -> anyhow::Result<()> {
    use commands::auth_session;

    let mut issues: Vec<String> = Vec::new();

    // 1. Session
    let session_ok = auth_session::load_session()
        .is_some_and(|s| {
            s.expires_at > std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });
    if !session_ok {
        issues.push("Session locked. Fix: logline auth unlock".into());
    }

    // 2. Auth identity
    let identity = auth_session::load_identity();
    let logged_in = identity.is_some();
    let passkey_ok = identity.as_ref().is_some_and(|i| i.auth_method == "passkey");
    let founder_blocked = identity.as_ref().is_some_and(|i| i.is_founder);

    if !logged_in {
        issues.push("Not logged in. Fix: logline auth login --passkey".into());
    } else if !passkey_ok {
        issues.push(format!(
            "Auth method is '{}', must be 'passkey'. Fix: logline auth login --passkey",
            identity.as_ref().map(|i| i.auth_method.as_str()).unwrap_or("?")
        ));
    }
    if founder_blocked {
        issues.push("Founder/god mode blocked for infra. Fix: use operator/service account.".into());
    }

    // 3. Pipeline exists
    let pipeline_file = std::env::current_dir()
        .unwrap_or_default()
        .join("logline.cicd.json");
    let pipeline_exists = if pipeline_file.exists() {
        let content = std::fs::read_to_string(&pipeline_file).unwrap_or_default();
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&content);
        parsed
            .ok()
            .and_then(|v| v["pipelines"][pipeline].as_array().map(|a| !a.is_empty()))
            .unwrap_or(false)
    } else {
        false
    };
    if !pipeline_exists {
        issues.push(format!("Pipeline '{pipeline}' not found in logline.cicd.json"));
    }

    // 4. Key secrets
    let required_keys = ["database_url", "github_token", "vercel_token", "vercel_org_id", "vercel_project_id"];
    let mut missing_keys: Vec<&str> = Vec::new();
    for key in &required_keys {
        if secrets::load_credential(key).is_none() {
            missing_keys.push(key);
        }
    }
    if !missing_keys.is_empty() {
        issues.push(format!(
            "Missing secrets: {}. Fix: logline secrets set <key>",
            missing_keys.join(", ")
        ));
    }

    let ready = issues.is_empty();

    let report = serde_json::json!({
        "ready": ready,
        "pipeline": pipeline,
        "session_active": session_ok,
        "logged_in": logged_in,
        "passkey_ok": passkey_ok,
        "founder_blocked": founder_blocked,
        "pipeline_exists": pipeline_exists,
        "missing_secrets": missing_keys,
        "issues": issues,
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Pre-flight: {pipeline}\n");

    let items: &[(&str, bool)] = &[
        ("session", session_ok),
        ("logged_in", logged_in),
        ("passkey", passkey_ok),
        ("non-founder", !founder_blocked),
        ("pipeline", pipeline_exists),
        ("secrets", missing_keys.is_empty()),
    ];

    for (name, ok) in items {
        let mark = if *ok { "✓" } else { "✗" };
        println!("  {mark} {name}");
    }

    println!();
    if ready {
        println!("Ready. Run: logline cicd run --pipeline {pipeline}");
    } else {
        for issue in &issues {
            println!("  ✗ {issue}");
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn get_hostname() -> String {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.is_empty() {
            return h;
        }
    }
    if let Ok(h) = fs::read_to_string("/etc/hostname") {
        let trimmed = h.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    "logline-cli".to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn chrono_now() -> String {
    let secs = now_secs();
    format!("1970-01-01T00:00:00Z")
        .replace("1970-01-01T00:00:00Z", &format_timestamp(secs))
}

fn format_timestamp(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;

    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

pub fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year { break; }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let months: [u64; 12] = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0;
    for m in months {
        if days < m { break; }
        days -= m;
        month += 1;
    }
    (year, month + 1, days + 1)
}

fn is_leap(y: u64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s.find('=').ok_or_else(|| "must be KEY=VALUE".to_string())?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

/// Gate: require an active unlocked session. Used by command modules.
pub fn require_unlocked() -> anyhow::Result<commands::auth_session::SessionToken> {
    commands::auth_session::require_unlocked()
}

/// Uber-gate: session + passkey + non-founder. Used by deploy/cicd/db commands.
pub fn require_infra_identity() -> anyhow::Result<(commands::auth_session::SessionToken, commands::auth_session::AuthIdentity)> {
    commands::auth_session::require_infra_identity()
}

pub fn pout(json_mode: bool, value: serde_json::Value, text: &str) -> anyhow::Result<()> {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{text}");
    }
    Ok(())
}

// ─── Supabase CLI helpers ───────────────────────────────────────────────────

fn run_supabase_stream(args: &[&str], workdir: Option<&PathBuf>) -> anyhow::Result<()> {
    let mut cmd = Command::new("supabase");
    if let Some(wd) = workdir {
        cmd.arg("--workdir").arg(wd);
    }
    apply_supabase_env(&mut cmd, workdir);
    cmd.args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::inherit());

    let status = cmd.status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("supabase CLI not found. Install with `brew install supabase/tap/supabase`")
        } else {
            anyhow::anyhow!(e)
        }
    })?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("supabase command failed with status {status}");
    }
}

fn apply_supabase_env(cmd: &mut Command, _workdir: Option<&PathBuf>) {
    let has_access = std::env::var("SUPABASE_ACCESS_TOKEN")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    if has_access {
        return;
    }

    if let Ok(entry) = keyring::Entry::new("logline-cli", "supabase_access_token") {
        if let Ok(token) = entry.get_password() {
            cmd.env("SUPABASE_ACCESS_TOKEN", token);
            return;
        }
    }

    eprintln!("Warning: No SUPABASE_ACCESS_TOKEN found in keychain or env.");
    eprintln!("  Store it with: logline supabase store-token");
}

