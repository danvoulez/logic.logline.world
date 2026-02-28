use clap::Subcommand;

use crate::integrations::{github, supabase_migrate, vercel};

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = crate::days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn receipt_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("rcpt-{nanos:x}")
}

#[derive(Debug, Subcommand)]
pub enum DeployCommands {
    /// Deploy everything: Supabase -> GitHub -> Vercel (with gates)
    All {
        #[arg(long, default_value = "production")]
        env: String,
    },
    /// Deploy Supabase: run migrations + verify RLS
    Supabase {
        #[arg(long, default_value = "production")]
        env: String,
    },
    /// Deploy to GitHub: git push (and optionally create PR or release)
    Github {
        /// Create a PR instead of pushing directly
        #[arg(long)]
        pr: bool,
        /// PR title (required with --pr)
        #[arg(long)]
        title: Option<String>,
        /// Base branch for PR
        #[arg(long, default_value = "main")]
        base: String,
        /// Create a release with this tag
        #[arg(long)]
        tag: Option<String>,
        /// Release notes
        #[arg(long)]
        notes: Option<String>,
    },
    /// Deploy to Vercel: sync env vars + poll deployment
    Vercel {
        #[arg(long, default_value = "production")]
        env: String,
    },
}

pub fn cmd_deploy(command: DeployCommands, json: bool) -> anyhow::Result<()> {
    crate::require_infra_identity()?;

    match command {
        DeployCommands::All { env } => cmd_deploy_all(&env, json),
        DeployCommands::Supabase { env } => cmd_deploy_supabase(&env, json),
        DeployCommands::Github {
            pr,
            title,
            base,
            tag,
            notes,
        } => cmd_deploy_github(pr, title.as_deref(), &base, tag.as_deref(), notes.as_deref(), json),
        DeployCommands::Vercel { env } => cmd_deploy_vercel(&env, json),
    }
}

fn cmd_deploy_all(env: &str, json: bool) -> anyhow::Result<()> {
    let started_at = now_iso();
    let rid = receipt_id();
    let mut gates: Vec<serde_json::Value> = Vec::new();

    eprintln!("[1/7] Verifying identity ..........");
    let (session, identity) = crate::require_infra_identity()?;
    gates.push(serde_json::json!({
        "gate": "auth_identity",
        "passed": true,
        "session_id": session.session_id,
        "user_id": identity.user_id,
        "auth_method": identity.auth_method,
        "profile": identity.profile,
    }));
    eprintln!("  ✓ {} ({}, {})", identity.email.as_deref().unwrap_or("?"), identity.auth_method, identity.profile);

    eprintln!("[2/7] Migrating Supabase ..........");
    supabase_migrate::migrate_up(env, false)?;
    gates.push(serde_json::json!({"gate": "db_migrate", "passed": true}));
    eprintln!("  ✓ Migrations applied");

    eprintln!("[3/7] Verifying RLS ...............");
    supabase_migrate::verify_rls(env, false)?;
    gates.push(serde_json::json!({"gate": "rls_verify", "passed": true}));
    eprintln!("  ✓ RLS verified");

    eprintln!("[4/7] Pushing to GitHub ...........");
    let push_result = github::git_push()?;
    let branch = push_result["branch"].as_str().unwrap_or("?");
    let sha = push_result["commit_sha"].as_str().unwrap_or("?");
    gates.push(serde_json::json!({"gate": "git_push", "passed": true, "sha": sha}));
    eprintln!("  ✓ Pushed {branch} ({sha})");

    eprintln!("[5/7] Deploying Vercel ............");
    let deploy_result = vercel::poll_deployment()?;
    let deploy_url = deploy_result["url"].as_str().unwrap_or("?").to_string();
    gates.push(serde_json::json!({"gate": "vercel_deploy", "passed": true, "url": deploy_url}));
    eprintln!("\n  ✓ {deploy_url}");

    eprintln!("[6/7] Health check ................");
    let health_ok = health_check(&deploy_url);
    gates.push(serde_json::json!({"gate": "health_check", "passed": health_ok}));
    if health_ok {
        eprintln!("  ✓ 200 OK");
    } else {
        eprintln!("  ⚠ Health check failed (non-blocking)");
    }

    let ended_at = now_iso();

    let receipt = serde_json::json!({
        "ok": true,
        "receipt_id": rid,
        "env": env,
        "started_at": started_at,
        "ended_at": ended_at,
        "principal": {
            "user_id": identity.user_id,
            "email": identity.email,
            "auth_method": identity.auth_method,
            "profile": identity.profile,
        },
        "gates": gates,
        "push": push_result,
        "deploy": deploy_result,
        "health": health_ok,
    });

    if let Ok(receipt_str) = serde_json::to_string_pretty(&receipt) {
        let _ = std::fs::write("receipt.json", &receipt_str);
    }

    eprintln!();
    crate::pout(
        json,
        receipt,
        &format!("Deployed to {env}. Production live at {deploy_url}"),
    )
}

fn cmd_deploy_supabase(env: &str, json: bool) -> anyhow::Result<()> {
    eprintln!("Deploying Supabase ({env})...");
    supabase_migrate::migrate_up(env, false)?;
    supabase_migrate::verify_rls(env, false)?;
    crate::pout(
        json,
        serde_json::json!({"ok": true, "target": "supabase", "env": env}),
        &format!("Supabase deploy complete ({env}): migrations applied, RLS verified."),
    )
}

fn cmd_deploy_github(
    pr: bool,
    title: Option<&str>,
    base: &str,
    tag: Option<&str>,
    notes: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    if pr {
        let t = title.unwrap_or("PR from logline deploy");
        eprintln!("Creating PR: {t}...");
        let result = github::create_pr(t, "", base)?;
        let pr_url = result["pr_url"].as_str().unwrap_or("?").to_string();
        return crate::pout(
            json,
            result,
            &format!("PR created: {pr_url}"),
        );
    }

    eprintln!("Pushing to GitHub...");
    let result = github::git_push()?;
    let branch = result["branch"].as_str().unwrap_or("?");
    eprintln!("  ✓ Pushed {branch}");

    if let Some(tag) = tag {
        eprintln!("Creating release {tag}...");
        let release = github::create_release(tag, notes)?;
        let url = release["release_url"].as_str().unwrap_or("?");
        eprintln!("  ✓ Release: {url}");
    }

    crate::pout(json, result, "GitHub deploy complete.")
}

fn cmd_deploy_vercel(env: &str, json: bool) -> anyhow::Result<()> {
    eprintln!("Deploying Vercel ({env})...");

    eprintln!("  Syncing env vars...");
    let sync_result = vercel::sync_env()?;
    let synced = sync_result["synced"].as_u64().unwrap_or(0);
    eprintln!("  ✓ {synced} env var(s) synced");

    eprintln!("  Waiting for deployment...");
    let deploy = vercel::poll_deployment()?;
    let url = deploy["url"].as_str().unwrap_or("?");

    crate::pout(
        json,
        serde_json::json!({
            "ok": true,
            "target": "vercel",
            "env": env,
            "env_synced": synced,
            "deploy": deploy,
        }),
        &format!("Vercel deploy complete ({env}): {url}"),
    )
}

fn health_check(url: &str) -> bool {
    let target = if url.starts_with("http") {
        format!("{url}/api/panels")
    } else {
        format!("https://{url}/api/panels")
    };

    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()
        .and_then(|c| c.get(&target).send().ok())
        .is_some_and(|r| r.status().is_success())
}
