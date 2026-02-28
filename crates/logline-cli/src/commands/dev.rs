use std::process::{Command, Stdio};

use clap::Subcommand;

use crate::commands::secrets;

#[derive(Debug, Subcommand)]
pub enum DevCommands {
    /// Build the Next.js app (npm run build with DATABASE_URL injected)
    Build,
    /// Start the dev server (npm run dev with DATABASE_URL injected)
    Start,
    /// Push Drizzle schema to database (drizzle-kit push with creds injected)
    MigratePush,
    /// Generate Drizzle migration files (drizzle-kit generate with creds injected)
    MigrateGenerate,
}

fn run_with_db_env(program: &str, args: &[&str], use_unpooled: bool) -> anyhow::Result<()> {
    let db_url = if use_unpooled {
        secrets::require_credential_or_env("database_url_unpooled", "LOGLINE_DATABASE_URL_UNPOOLED")?
    } else {
        secrets::require_credential_or_env("database_url", "LOGLINE_DATABASE_URL")?
    };

    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("DATABASE_URL", &db_url)
        .env("DATABASE_URL_UNPOOLED", &db_url)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::inherit());

    if let Some(url) = secrets::load_credential("supabase_url") {
        cmd.env("NEXT_PUBLIC_SUPABASE_URL", url);
    }
    if let Some(key) = secrets::load_credential("supabase_anon_key") {
        cmd.env("NEXT_PUBLIC_SUPABASE_ANON_KEY", key);
    }

    let status = cmd.status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("'{program}' not found. Is it installed?")
        } else {
            anyhow::anyhow!("Failed to run '{program}': {e}")
        }
    })?;

    if !status.success() {
        anyhow::bail!("{program} exited with {status}");
    }
    Ok(())
}

pub fn cmd_dev(command: DevCommands, _json: bool) -> anyhow::Result<()> {
    crate::require_unlocked()?;

    match command {
        DevCommands::Build => {
            eprintln!("Building Next.js app (DATABASE_URL injected from keychain)...");
            run_with_db_env("npm", &["run", "build"], false)
        }
        DevCommands::Start => {
            eprintln!("Starting dev server (DATABASE_URL injected from keychain)...");
            run_with_db_env("npm", &["run", "dev"], false)
        }
        DevCommands::MigratePush => {
            eprintln!("Pushing Drizzle schema (DATABASE_URL_UNPOOLED injected from keychain)...");
            run_with_db_env("npx", &["drizzle-kit", "push"], true)
        }
        DevCommands::MigrateGenerate => {
            eprintln!("Generating Drizzle migrations (DATABASE_URL_UNPOOLED injected from keychain)...");
            run_with_db_env("npx", &["drizzle-kit", "generate"], true)
        }
    }
}
