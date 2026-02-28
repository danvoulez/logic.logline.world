use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::bail;
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::commands::secrets;

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

fn cicd_receipt_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("cicd-{nanos:x}")
}

const PIPELINE_SECRETS: &[(&str, &str)] = &[
    ("database_url", "LOGLINE_DATABASE_URL"),
    ("database_url_unpooled", "LOGLINE_DATABASE_URL_UNPOOLED"),
    ("github_token", "LOGLINE_GITHUB_TOKEN"),
    ("vercel_token", "LOGLINE_VERCEL_TOKEN"),
    ("vercel_org_id", "LOGLINE_VERCEL_ORG_ID"),
    ("vercel_project_id", "LOGLINE_VERCEL_PROJECT_ID"),
    ("supabase_url", "LOGLINE_SUPABASE_URL"),
    ("supabase_anon_key", "LOGLINE_SUPABASE_ANON_KEY"),
];

#[derive(Debug, Subcommand)]
pub enum CicdCommands {
    /// Run the CI/CD pipeline defined in logline.cicd.json
    Run {
        /// Pipeline name (default: first pipeline in file)
        #[arg(long)]
        pipeline: Option<String>,
        /// Run only a single step
        #[arg(long)]
        step: Option<String>,
        /// Show what would run without executing
        #[arg(long)]
        dry_run: bool,
        /// Non-interactive mode (for CI — reads creds from LOGLINE_* env vars)
        #[arg(long)]
        non_interactive: bool,
    },
    /// Show the status of the last pipeline run
    Status,
}

#[derive(Debug, Deserialize)]
struct PipelineFile {
    pipelines: HashMap<String, Vec<PipelineStep>>,
    #[serde(default = "default_on_failure")]
    on_failure: String,
    #[serde(default)]
    #[allow(dead_code)]
    artifacts: Vec<String>,
}

fn default_on_failure() -> String {
    "abort".into()
}

#[derive(Debug, Deserialize, Clone)]
struct PipelineStep {
    step: String,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    cmd: Option<String>,
}

#[derive(Debug, Serialize)]
struct StepResult {
    step: String,
    status: String,
    elapsed_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn cmd_cicd(command: CicdCommands, json: bool) -> anyhow::Result<()> {
    match command {
        CicdCommands::Run {
            pipeline,
            step,
            dry_run,
            non_interactive,
        } => cmd_cicd_run(pipeline.as_deref(), step.as_deref(), dry_run, non_interactive, json),
        CicdCommands::Status => cmd_cicd_status(json),
    }
}

fn load_pipeline_file() -> anyhow::Result<PipelineFile> {
    let path = std::env::current_dir()
        .unwrap_or_default()
        .join("logline.cicd.json");

    if !path.exists() {
        bail!(
            "logline.cicd.json not found in current directory.\n\
             Create it to define your CI/CD pipeline."
        );
    }

    let content = std::fs::read_to_string(&path)?;
    let file: PipelineFile = serde_json::from_str(&content)?;
    Ok(file)
}

fn cmd_cicd_run(
    pipeline_name: Option<&str>,
    single_step: Option<&str>,
    dry_run: bool,
    non_interactive: bool,
    json: bool,
) -> anyhow::Result<()> {
    let identity = if !non_interactive {
        let (_session, id) = crate::require_infra_identity()?;
        Some(id)
    } else {
        None
    };

    // Gate: block if migrations are pending (CI/CD must not silently skip schema changes)
    match crate::commands::db::get_pending_migration_names() {
        Ok(pending) if !pending.is_empty() => {
            bail!(
                "CI/CD blocked: {} pending migration(s):\n  {}\n\n\
                 Migrations must be reviewed and applied before CI/CD can run.\n\
                 Run:\n  logline db migrate review\n  logline db migrate apply --env prod",
                pending.len(),
                pending.join("\n  ")
            );
        }
        Ok(_) => {} // no pending migrations
        Err(e) => {
            eprintln!("WARNING: Could not check migration status: {e}");
            eprintln!("  Proceeding without migration gate.\n");
        }
    }

    let file = load_pipeline_file()?;

    let name = pipeline_name.unwrap_or_else(|| {
        file.pipelines.keys().next().map(|s| s.as_str()).unwrap_or("prod")
    });

    let steps = file
        .pipelines
        .get(name)
        .ok_or_else(|| {
            let available: Vec<_> = file.pipelines.keys().collect();
            anyhow::anyhow!(
                "Pipeline '{name}' not found. Available: {available:?}"
            )
        })?
        .clone();

    let steps_to_run: Vec<&PipelineStep> = if let Some(target) = single_step {
        let s = steps
            .iter()
            .find(|s| s.step == target)
            .ok_or_else(|| anyhow::anyhow!("Step '{target}' not found in pipeline '{name}'"))?;
        vec![s]
    } else {
        steps.iter().collect()
    };

    let total = steps_to_run.len();

    if dry_run {
        eprintln!("Pipeline: {name} (dry run — {total} steps)");
        for (i, step) in steps_to_run.iter().enumerate() {
            let cmd = step.run.as_deref().or(step.cmd.as_deref()).unwrap_or("?");
            eprintln!("  [{}/{}] {} — {cmd}", i + 1, total, step.step);
        }
        return crate::pout(
            json,
            serde_json::json!({"dry_run": true, "pipeline": name, "steps": total}),
            "Dry run complete. No changes made.",
        );
    }

    eprintln!("Pipeline: {name} ({total} steps)\n");

    let rid = cicd_receipt_id();
    let started_at = now_iso();
    let pipeline_start = Instant::now();
    let mut results: Vec<StepResult> = Vec::new();

    for (i, step) in steps_to_run.iter().enumerate() {
        let label = format!("[{}/{}] {}", i + 1, total, step.step);
        eprint!("{label:<40}");

        let step_start = Instant::now();

        let outcome = if let Some(shell_cmd) = &step.run {
            run_shell_command(shell_cmd)
        } else if let Some(logline_cmd) = &step.cmd {
            run_logline_command(logline_cmd, non_interactive)
        } else {
            Err(anyhow::anyhow!("Step '{}' has no 'run' or 'cmd'", step.step))
        };

        let elapsed = step_start.elapsed().as_millis();

        match outcome {
            Ok(()) => {
                eprintln!("✓ ({elapsed}ms)");
                results.push(StepResult {
                    step: step.step.clone(),
                    status: "ok".into(),
                    elapsed_ms: elapsed,
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("✗ ({elapsed}ms)");
                results.push(StepResult {
                    step: step.step.clone(),
                    status: "failed".into(),
                    elapsed_ms: elapsed,
                    error: Some(e.to_string()),
                });

                if file.on_failure == "abort" {
                    eprintln!("\nPipeline aborted at step '{}': {e}", step.step);

                    let principal = identity.as_ref().map(|id| serde_json::json!({
                        "user_id": id.user_id,
                        "email": id.email,
                        "auth_method": id.auth_method,
                        "profile": id.profile,
                    }));

                    let receipt = serde_json::json!({
                        "ok": false,
                        "receipt_id": rid,
                        "pipeline": name,
                        "principal": principal,
                        "started_at": started_at,
                        "ended_at": now_iso(),
                        "steps": results,
                        "aborted_at": step.step,
                        "total_ms": pipeline_start.elapsed().as_millis(),
                    });

                    write_receipt(&receipt);
                    if json {
                        println!("{}", serde_json::to_string_pretty(&receipt)?);
                    }
                    bail!("Pipeline '{name}' failed at step '{}'", step.step);
                }
            }
        }
    }

    let total_ms = pipeline_start.elapsed().as_millis();
    let all_ok = results.iter().all(|r| r.status == "ok");

    let principal = identity.as_ref().map(|id| serde_json::json!({
        "user_id": id.user_id,
        "email": id.email,
        "auth_method": id.auth_method,
        "profile": id.profile,
    }));

    let receipt = serde_json::json!({
        "ok": all_ok,
        "receipt_id": rid,
        "pipeline": name,
        "principal": principal,
        "started_at": started_at,
        "ended_at": now_iso(),
        "steps": results,
        "total_ms": total_ms,
    });

    write_receipt(&receipt);

    crate::pout(
        json,
        receipt,
        &format!(
            "\nPipeline: {name} — {} step(s) passed in {total_ms}ms.\nReceipt: receipt.json",
            results.len()
        ),
    )
}

fn cmd_cicd_status(json: bool) -> anyhow::Result<()> {
    let mut missing_secrets: Vec<&str> = Vec::new();
    let mut present_secrets: Vec<&str> = Vec::new();

    for &(key, env_var) in PIPELINE_SECRETS {
        if secrets::load_credential_or_env(key, env_var).is_some() {
            present_secrets.push(key);
        } else {
            missing_secrets.push(key);
        }
    }

    let pipelines_available: Vec<String> = load_pipeline_file()
        .ok()
        .map(|f| f.pipelines.keys().cloned().collect())
        .unwrap_or_default();

    let receipt_path = std::env::current_dir()
        .unwrap_or_default()
        .join("receipt.json");

    let last_receipt: Option<serde_json::Value> = receipt_path
        .exists()
        .then(|| {
            std::fs::read_to_string(&receipt_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
        .flatten();

    let report = serde_json::json!({
        "pipelines": pipelines_available,
        "secrets": {
            "present": present_secrets,
            "missing": missing_secrets,
        },
        "last_run": last_receipt,
        "ready": missing_secrets.is_empty() && !pipelines_available.is_empty(),
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Pipelines:");
    if pipelines_available.is_empty() {
        println!("  (none — create logline.cicd.json)");
    } else {
        for p in &pipelines_available {
            println!("  - {p}");
        }
    }

    println!("\nSecrets vault:");
    for &key in &present_secrets {
        println!("  ✓ {key}");
    }
    for &key in &missing_secrets {
        println!("  ✗ {key}  <-- run: logline secrets set {key}");
    }

    if let Some(receipt) = &last_receipt {
        let pipeline = receipt["pipeline"].as_str().unwrap_or("?");
        let ok = receipt["ok"].as_bool().unwrap_or(false);
        let total_ms = receipt["total_ms"].as_u64().unwrap_or(0);
        let rid = receipt["receipt_id"].as_str().unwrap_or("?");
        let status = if ok { "PASSED" } else { "FAILED" };
        println!("\nLast run: {pipeline} — {status} ({total_ms}ms) [{rid}]");

        if let Some(steps) = receipt["steps"].as_array() {
            for s in steps {
                let name = s["step"].as_str().unwrap_or("?");
                let st = s["status"].as_str().unwrap_or("?");
                let ms = s["elapsed_ms"].as_u64().unwrap_or(0);
                let mark = if st == "ok" { "✓" } else { "✗" };
                println!("  {mark} {name} ({ms}ms)");
            }
        }
    } else {
        println!("\nNo pipeline runs yet. Run: logline cicd run");
    }

    if !missing_secrets.is_empty() {
        println!("\n⚠ {} secret(s) missing — pipeline will fail.", missing_secrets.len());
    }

    Ok(())
}

fn run_shell_command(cmd: &str) -> anyhow::Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        bail!("Command failed: {cmd}")
    }
}

fn run_logline_command(cmd: &str, non_interactive: bool) -> anyhow::Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| "logline".into());
    let parts: Vec<&str> = cmd.split_whitespace().collect();

    let mut command = Command::new(&exe);
    command.args(&parts);
    command.arg("--json");

    if non_interactive {
        command.env("LOGLINE_NON_INTERACTIVE", "1");
    }

    command.stdout(Stdio::null()).stderr(Stdio::piped());

    let status = command
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run logline {cmd}: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        bail!("logline {cmd} failed")
    }
}

fn write_receipt(receipt: &serde_json::Value) {
    if let Ok(s) = serde_json::to_string_pretty(receipt) {
        let _ = std::fs::write("receipt.json", s);
    }
}
