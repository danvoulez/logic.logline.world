use std::path::PathBuf;

use anyhow::bail;
use clap::Subcommand;

use crate::commands::secrets;

#[derive(Debug, Subcommand)]
pub enum DbCommands {
    /// Execute a SQL query and return results as JSON
    Query {
        /// SQL statement
        sql: String,
    },
    /// List all tables with row counts
    Tables,
    /// Describe a table: columns, types, constraints
    Describe {
        /// Table name
        table: String,
    },
    /// Migration management
    Migrate {
        #[command(subcommand)]
        command: MigrateCommands,
    },
    /// Verify RLS is enabled and policies exist on all tables (mandatory gate)
    VerifyRls {
        /// Environment label (for output only)
        #[arg(long, default_value = "production")]
        env: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum MigrateCommands {
    /// Show migration status (applied vs pending)
    Status,
    /// Review pending migrations (generates diff, stores review receipt)
    Review,
    /// Apply pending migrations (requires recent review receipt + infra identity)
    Apply {
        /// Environment label
        #[arg(long, default_value = "production")]
        env: String,
    },
    /// [Legacy] Apply all pending migrations without review gate
    Up {
        /// Environment label
        #[arg(long, default_value = "production")]
        env: String,
    },
}

fn get_db_url() -> anyhow::Result<String> {
    if std::env::var("DATABASE_URL").ok().is_some_and(|v| !v.is_empty()) {
        eprintln!("WARNING: DATABASE_URL found in environment. This is a security risk.");
        eprintln!("  Store it in Keychain instead: logline secrets set database_url");
        eprintln!("  Then remove it from your environment / .env files.\n");
    }
    secrets::require_credential("database_url_unpooled")
        .or_else(|_| secrets::require_credential("database_url"))
        .map_err(|_| anyhow::anyhow!(
            "No database credentials found in Keychain.\n\
             Store with: logline secrets set database_url\n\
             Or:         logline secrets set database_url_unpooled"
        ))
}

pub fn cmd_db(command: DbCommands, json: bool) -> anyhow::Result<()> {
    crate::require_unlocked()?;

    match command {
        DbCommands::Query { sql } => cmd_db_query(&sql, json),
        DbCommands::Tables => cmd_db_tables(json),
        DbCommands::Describe { table } => cmd_db_describe(&table, json),
        DbCommands::Migrate { command: sub } => match sub {
            MigrateCommands::Status => cmd_migrate_status(json),
            MigrateCommands::Review => cmd_migrate_review(json),
            MigrateCommands::Apply { env } => cmd_migrate_apply(&env, json),
            MigrateCommands::Up { env } => cmd_migrate_up(&env, json),
        },
        DbCommands::VerifyRls { env } => cmd_verify_rls(&env, json),
    }
}

fn cmd_db_query(sql: &str, json: bool) -> anyhow::Result<()> {
    let url = get_db_url()?;
    let output = std::process::Command::new("psql")
        .arg(&url)
        .arg("-c")
        .arg(sql)
        .arg("--no-psqlrc")
        .args(if json { vec!["--tuples-only", "--csv"] } else { vec![] })
        .env("PGCONNECT_TIMEOUT", "10")
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run psql: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Query failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if json {
        let rows: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        print!("{stdout}");
    }
    Ok(())
}

fn cmd_db_tables(json: bool) -> anyhow::Result<()> {
    let sql = r"
        SELECT schemaname, tablename,
               pg_stat_get_live_tuples(c.oid) AS row_count
        FROM pg_tables t
        JOIN pg_class c ON c.relname = t.tablename
        JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = t.schemaname
        WHERE schemaname IN ('public', 'app')
        ORDER BY schemaname, tablename;
    ";
    cmd_db_query(sql.trim(), json)
}

fn cmd_db_describe(table: &str, json: bool) -> anyhow::Result<()> {
    let sql = format!(
        r"
        SELECT column_name, data_type, is_nullable, column_default
        FROM information_schema.columns
        WHERE table_name = '{table}'
        ORDER BY ordinal_position;
        "
    );
    cmd_db_query(sql.trim(), json)
}

fn cmd_migrate_status(json: bool) -> anyhow::Result<()> {
    let migrations_dir = find_migrations_dir()?;
    let files = list_migration_files(&migrations_dir)?;

    let url = get_db_url()?;
    let applied = get_applied_migrations(&url)?;

    let mut statuses = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let is_applied = applied.contains(&name);
        statuses.push(serde_json::json!({
            "migration": name,
            "applied": is_applied,
        }));
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&statuses)?);
    } else {
        for s in &statuses {
            let name = s["migration"].as_str().unwrap_or("?");
            let applied = s["applied"].as_bool().unwrap_or(false);
            let mark = if applied { "✓" } else { "PENDING" };
            println!("  {mark:<8} {name}");
        }
    }
    Ok(())
}

fn cmd_migrate_up(env: &str, json: bool) -> anyhow::Result<()> {
    let migrations_dir = find_migrations_dir()?;
    let files = list_migration_files(&migrations_dir)?;
    let url = get_db_url()?;

    ensure_migrations_table(&url)?;
    let applied = get_applied_migrations(&url)?;

    let pending: Vec<_> = files
        .iter()
        .filter(|f| {
            let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
            !applied.contains(&name)
        })
        .collect();

    if pending.is_empty() {
        return crate::pout(
            json,
            serde_json::json!({"ok": true, "applied": 0, "env": env}),
            "All migrations already applied.",
        );
    }

    eprintln!("Applying {} pending migration(s) to {env}...", pending.len());

    let mut applied_count = 0u32;
    for file in &pending {
        let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let sql = std::fs::read_to_string(file)?;

        eprintln!("  Applying: {name}...");
        let full_sql = format!(
            "BEGIN;\n{sql}\nINSERT INTO _logline_migrations (name) VALUES ('{name}');\nCOMMIT;\n"
        );

        let output = std::process::Command::new("psql")
            .arg(&url)
            .arg("-c")
            .arg(&full_sql)
            .arg("--no-psqlrc")
            .env("PGCONNECT_TIMEOUT", "30")
            .output()
            .map_err(|e| anyhow::anyhow!("psql failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Migration '{name}' failed: {stderr}");
        }

        eprintln!("  ✓ {name}");
        applied_count += 1;
    }

    crate::pout(
        json,
        serde_json::json!({"ok": true, "applied": applied_count, "env": env}),
        &format!("{applied_count} migration(s) applied to {env}."),
    )
}

const REVIEW_RECEIPT_KEY: &str = "logline_migrate_review_receipt";
const REVIEW_RECEIPT_TTL_SECS: u64 = 3600; // 1 hour

fn cmd_migrate_review(json: bool) -> anyhow::Result<()> {
    let migrations_dir = find_migrations_dir()?;
    let files = list_migration_files(&migrations_dir)?;
    let url = get_db_url()?;

    ensure_migrations_table(&url)?;
    let applied = get_applied_migrations(&url)?;

    let pending: Vec<_> = files
        .iter()
        .filter(|f| {
            let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
            !applied.contains(&name)
        })
        .collect();

    if pending.is_empty() {
        return crate::pout(
            json,
            serde_json::json!({"ok": true, "pending": 0, "review": "nothing_to_review"}),
            "No pending migrations to review.",
        );
    }

    eprintln!("Reviewing {} pending migration(s):\n", pending.len());

    let mut pending_names: Vec<String> = Vec::new();
    for file in &pending {
        let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let sql = std::fs::read_to_string(file)?;

        eprintln!("━━━ {} ━━━", name);
        eprintln!("{sql}");
        eprintln!();
        pending_names.push(name);
    }

    // Try to open DataGrip if available
    let mut datagrip_opened = false;
    for file in &pending {
        let result = std::process::Command::new("open")
            .arg("-a")
            .arg("DataGrip")
            .arg(file)
            .output();
        if result.is_ok_and(|o| o.status.success()) {
            datagrip_opened = true;
        }
    }

    if datagrip_opened {
        eprintln!("Opened migration file(s) in DataGrip for review.");
    }

    // Store review receipt in Keychain
    let now = now_secs();
    let receipt = serde_json::json!({
        "reviewed_at": now,
        "expires_at": now + REVIEW_RECEIPT_TTL_SECS,
        "migrations": pending_names,
    });
    secrets::store_credential(REVIEW_RECEIPT_KEY, &serde_json::to_string(&receipt)?)?;

    eprintln!("Review receipt stored (valid for 1 hour).");
    eprintln!("To apply: logline db migrate apply --env prod");

    crate::pout(
        json,
        serde_json::json!({
            "ok": true,
            "pending": pending_names.len(),
            "migrations": pending_names,
            "review_expires_at": now + REVIEW_RECEIPT_TTL_SECS,
            "datagrip": datagrip_opened,
        }),
        &format!("{} migration(s) reviewed. Receipt valid until {}.",
            pending_names.len(),
            format_ttl_remaining(REVIEW_RECEIPT_TTL_SECS)),
    )
}

fn cmd_migrate_apply(env: &str, json: bool) -> anyhow::Result<()> {
    // Gate 1: require infra identity (Touch ID + passkey + non-founder)
    let (_session, identity) = crate::require_infra_identity()?;
    eprintln!("Identity: {} ({})", identity.email.as_deref().unwrap_or("?"), identity.profile);

    // Gate 2: require recent review receipt
    let receipt_json = secrets::load_credential(REVIEW_RECEIPT_KEY)
        .ok_or_else(|| anyhow::anyhow!(
            "No review receipt found.\n\
             You must review migrations before applying them.\n\
             Run: logline db migrate review"
        ))?;
    let receipt: serde_json::Value = serde_json::from_str(&receipt_json)
        .map_err(|_| anyhow::anyhow!("Corrupt review receipt. Run: logline db migrate review"))?;

    let expires_at = receipt["expires_at"].as_u64().unwrap_or(0);
    let now = now_secs();
    if now > expires_at {
        bail!(
            "Review receipt expired ({} ago).\n\
             Re-review the migrations: logline db migrate review",
            format_ttl_remaining(now - expires_at)
        );
    }

    let reviewed_migrations: Vec<String> = receipt["migrations"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    eprintln!("Review receipt valid. Reviewed: {}", reviewed_migrations.join(", "));

    // Apply migrations (reuses existing logic)
    cmd_migrate_up(env, json)?;

    // Invalidate the review receipt after successful apply
    let _ = secrets::store_credential(REVIEW_RECEIPT_KEY,
        &serde_json::to_string(&serde_json::json!({"consumed": true, "consumed_at": now}))?);

    // Auto-run RLS verification
    eprintln!("\nPost-migration RLS verification...");
    cmd_verify_rls(env, json)?;

    Ok(())
}

fn format_ttl_remaining(secs: u64) -> String {
    if secs < 60 { return format!("{secs}s"); }
    let mins = secs / 60;
    if mins < 60 { return format!("{mins}m"); }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    format!("{hours}h {rem_mins}m")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Check for pending migrations. Used by cicd to block if migrations haven't been applied.
pub fn get_pending_migration_names() -> anyhow::Result<Vec<String>> {
    let migrations_dir = find_migrations_dir()?;
    let files = list_migration_files(&migrations_dir)?;
    let url = get_db_url()?;

    ensure_migrations_table(&url)?;
    let applied = get_applied_migrations(&url)?;

    Ok(files
        .iter()
        .filter_map(|f| {
            let name = f.file_name()?.to_string_lossy().to_string();
            if applied.contains(&name) { None } else { Some(name) }
        })
        .collect())
}

const SENSITIVE_TABLES: &[&str] = &[
    "tenant_memberships",
    "app_memberships",
    "tenant_email_allowlist",
    "user_capabilities",
    "app_service_config",
    "cli_passkey_credentials",
];

const APPEND_ONLY_TABLES: &[&str] = &["fuel_events"];

fn cmd_verify_rls(env: &str, json: bool) -> anyhow::Result<()> {
    let url = get_db_url()?;
    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    let mut tables_checked = 0u32;

    // Gate 1: RLS enabled + policy count on all tables
    let rls_sql = r"
        SELECT t.schemaname, t.tablename, c.relrowsecurity AS rls_enabled,
               (SELECT count(*) FROM pg_policies p WHERE p.tablename = t.tablename AND p.schemaname = t.schemaname) AS policy_count
        FROM pg_tables t
        JOIN pg_class c ON c.relname = t.tablename
        JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = t.schemaname
        WHERE t.schemaname IN ('public', 'app')
        ORDER BY t.schemaname, t.tablename;
    ";

    let output = run_psql_query(&url, rls_sql)?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 4 { continue; }
        let schema = parts[0].trim();
        let table = parts[1].trim();
        let rls_on = parts[2].trim() == "t";
        let policy_count: u32 = parts[3].trim().parse().unwrap_or(0);
        tables_checked += 1;

        let full_name = format!("{schema}.{table}");

        if !rls_on {
            issues.push(serde_json::json!({
                "table": full_name,
                "severity": "critical",
                "issue": "RLS not enabled",
                "fix": format!("ALTER TABLE {full_name} ENABLE ROW LEVEL SECURITY;"),
            }));
        }
        if policy_count == 0 {
            issues.push(serde_json::json!({
                "table": full_name,
                "severity": "critical",
                "issue": "No RLS policies defined",
                "fix": format!("Add at least one policy for {full_name}"),
            }));
        }

        if SENSITIVE_TABLES.contains(&table) && policy_count < 2 {
            warnings.push(serde_json::json!({
                "table": full_name,
                "severity": "warning",
                "issue": format!("Sensitive table has only {policy_count} policy(ies) — expected >= 2 (SELECT + mutation)"),
                "fix": format!("Review policies for {full_name}: needs separate SELECT and INSERT/UPDATE policies"),
            }));
        }
    }

    // Gate 2: fuel_events must be append-only (no UPDATE/DELETE grants or policies)
    for &table in APPEND_ONLY_TABLES {
        let grant_sql = format!(
            r"SELECT privilege_type FROM information_schema.role_table_grants
              WHERE table_name = '{table}' AND grantee IN ('authenticated', 'anon', 'public')
              AND privilege_type IN ('UPDATE', 'DELETE');"
        );

        if let Ok(out) = run_psql_query(&url, &grant_sql) {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let priv_type = line.trim();
                if priv_type == "UPDATE" || priv_type == "DELETE" {
                    issues.push(serde_json::json!({
                        "table": table,
                        "severity": "critical",
                        "issue": format!("{priv_type} grant exists on append-only table"),
                        "fix": format!("REVOKE {priv_type} ON {table} FROM authenticated, anon, public;"),
                    }));
                }
            }
        }

        let policy_sql = format!(
            r"SELECT polname, polcmd FROM pg_policies WHERE tablename = '{table}' AND polcmd IN ('w', 'd');"
        );
        if let Ok(out) = run_psql_query(&url, &policy_sql) {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() { continue; }
                let cmd_char = line.rsplit(',').next().unwrap_or("").trim();
                let cmd_name = match cmd_char {
                    "w" => "UPDATE",
                    "d" => "DELETE",
                    _ => continue,
                };
                issues.push(serde_json::json!({
                    "table": table,
                    "severity": "critical",
                    "issue": format!("{cmd_name} policy exists on append-only table"),
                    "fix": format!("DROP the {cmd_name} policy on {table} — append-only tables must not allow modifications"),
                }));
            }
        }
    }

    // Gate 3: SECURITY DEFINER functions must have SET search_path
    let definer_sql = r"
        SELECT n.nspname, p.proname, p.prosecdef,
               (p.proconfig IS NOT NULL AND array_to_string(p.proconfig, ',') LIKE '%search_path%') AS has_search_path
        FROM pg_proc p
        JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname IN ('public', 'app')
          AND p.prosecdef = true;
    ";

    if let Ok(out) = run_psql_query(&url, definer_sql) {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() < 4 { continue; }
            let schema = parts[0].trim();
            let func = parts[1].trim();
            let has_path = parts[3].trim() == "t";

            if !has_path {
                issues.push(serde_json::json!({
                    "table": format!("{schema}.{func}()"),
                    "severity": "critical",
                    "issue": "SECURITY DEFINER function without SET search_path",
                    "fix": format!("ALTER FUNCTION {schema}.{func} SET search_path = {schema};"),
                }));
            }
        }
    }

    // Report
    let report = serde_json::json!({
        "ok": issues.is_empty(),
        "env": env,
        "tables_checked": tables_checked,
        "failures": issues,
        "warnings": warnings,
    });

    if issues.is_empty() {
        let warn_count = warnings.len();
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            eprintln!("RLS verification PASSED ({env}): {tables_checked} tables secured.");
            if warn_count > 0 {
                eprintln!("  {warn_count} warning(s):");
                for w in &warnings {
                    eprintln!("    ⚠ {} — {}", w["table"].as_str().unwrap_or("?"), w["issue"].as_str().unwrap_or("?"));
                }
            }
        }
        Ok(())
    } else {
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            eprintln!("RLS verification FAILED ({env}):");
            for issue in &issues {
                eprintln!(
                    "  ✗ [{}] {} — {}",
                    issue["severity"].as_str().unwrap_or("?"),
                    issue["table"].as_str().unwrap_or("?"),
                    issue["issue"].as_str().unwrap_or("?")
                );
                eprintln!("    Fix: {}", issue["fix"].as_str().unwrap_or("?"));
            }
            if !warnings.is_empty() {
                eprintln!("  Warnings:");
                for w in &warnings {
                    eprintln!("    ⚠ {} — {}", w["table"].as_str().unwrap_or("?"), w["issue"].as_str().unwrap_or("?"));
                }
            }
        }
        bail!("RLS verification failed: {} critical issue(s), {} warning(s). Deploy blocked.", issues.len(), warnings.len())
    }
}

fn run_psql_query(url: &str, sql: &str) -> anyhow::Result<std::process::Output> {
    let output = std::process::Command::new("psql")
        .arg(url)
        .arg("-c")
        .arg(sql)
        .arg("--no-psqlrc")
        .arg("--tuples-only")
        .arg("--csv")
        .env("PGCONNECT_TIMEOUT", "10")
        .output()
        .map_err(|e| anyhow::anyhow!("psql failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Query failed: {stderr}");
    }
    Ok(output)
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn find_migrations_dir() -> anyhow::Result<PathBuf> {
    let candidates = [
        std::env::current_dir()
            .unwrap_or_default()
            .join("supabase/migrations"),
        std::env::current_dir()
            .unwrap_or_default()
            .parent()
            .map(|p| p.join("supabase/migrations"))
            .unwrap_or_default(),
    ];

    for dir in &candidates {
        if dir.is_dir() {
            return Ok(dir.clone());
        }
    }
    bail!("Cannot find supabase/migrations/ directory")
}

fn list_migration_files(dir: &PathBuf) -> anyhow::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();
    Ok(files)
}

fn ensure_migrations_table(url: &str) -> anyhow::Result<()> {
    let sql = r"
        CREATE TABLE IF NOT EXISTS _logline_migrations (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at TIMESTAMPTZ DEFAULT now()
        );
    ";
    let output = std::process::Command::new("psql")
        .arg(url)
        .arg("-c")
        .arg(sql)
        .arg("--no-psqlrc")
        .env("PGCONNECT_TIMEOUT", "10")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to create migrations table: {stderr}");
    }
    Ok(())
}

fn get_applied_migrations(url: &str) -> anyhow::Result<Vec<String>> {
    ensure_migrations_table(url)?;

    let sql = "SELECT name FROM _logline_migrations ORDER BY name;";
    let output = std::process::Command::new("psql")
        .arg(url)
        .arg("-c")
        .arg(sql)
        .arg("--no-psqlrc")
        .arg("--tuples-only")
        .arg("--no-align")
        .env("PGCONNECT_TIMEOUT", "10")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to query migrations: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}
