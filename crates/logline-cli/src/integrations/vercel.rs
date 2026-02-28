use std::time::Duration;

use anyhow::bail;

use crate::commands::secrets;

fn vercel_client() -> anyhow::Result<(reqwest::blocking::Client, String, String, String)> {
    let token = secrets::require_credential_or_env("vercel_token", "LOGLINE_VERCEL_TOKEN")?;
    let org_id = secrets::require_credential_or_env("vercel_org_id", "VERCEL_ORG_ID")?;
    let project_id =
        secrets::require_credential_or_env("vercel_project_id", "VERCEL_PROJECT_ID")?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    Ok((client, token, org_id, project_id))
}

/// Poll Vercel for the latest deployment and wait until it's READY or ERROR.
pub fn poll_deployment() -> anyhow::Result<serde_json::Value> {
    let (client, token, _org_id, project_id) = vercel_client()?;

    eprintln!("Waiting for Vercel deployment...");

    let max_wait = Duration::from_secs(300);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            bail!("Vercel deployment timed out after 5 minutes");
        }

        let url = format!(
            "https://api.vercel.com/v6/deployments?projectId={project_id}&limit=1&target=production"
        );

        let resp = client
            .get(&url)
            .bearer_auth(&token)
            .header("User-Agent", "logline-cli")
            .send()?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            bail!("Vercel API error ({status}): {text}");
        }

        let body: serde_json::Value = resp.json()?;
        if let Some(deployments) = body["deployments"].as_array() {
            if let Some(deploy) = deployments.first() {
                let state = deploy["state"].as_str().unwrap_or("UNKNOWN");
                let deploy_url = deploy["url"].as_str().unwrap_or("?");
                let deploy_id = deploy["uid"].as_str().unwrap_or("?");

                match state {
                    "READY" => {
                        return Ok(serde_json::json!({
                            "ok": true,
                            "deployment_id": deploy_id,
                            "url": format!("https://{deploy_url}"),
                            "status": "READY",
                        }));
                    }
                    "ERROR" | "CANCELED" => {
                        bail!("Vercel deployment failed: {state}");
                    }
                    _ => {
                        eprint!(".");
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_secs(5));
    }
}

/// Set an environment variable on Vercel via API.
pub fn set_env_var(key: &str, value: &str, target: &[&str]) -> anyhow::Result<()> {
    let (client, token, _org_id, project_id) = vercel_client()?;

    let url = format!("https://api.vercel.com/v10/projects/{project_id}/env");

    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .header("User-Agent", "logline-cli")
        .json(&serde_json::json!({
            "key": key,
            "value": value,
            "target": target,
            "type": "encrypted",
        }))
        .send()?;

    if resp.status().is_success() || resp.status().as_u16() == 409 {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("Vercel env set failed ({status}): {text}")
    }
}

/// Sync env vars from a manifest file (vercel.env.json) to Vercel.
pub fn sync_env() -> anyhow::Result<serde_json::Value> {
    let manifest_path = std::env::current_dir()
        .unwrap_or_default()
        .join("vercel.env.json");

    if !manifest_path.exists() {
        return Ok(serde_json::json!({"ok": true, "synced": 0, "reason": "no manifest"}));
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let entries = manifest
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("vercel.env.json must be a JSON object"))?;

    let mut synced = 0u32;
    for (vercel_key, config) in entries {
        let source = config["from_secret"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'from_secret' for key '{vercel_key}'"))?;

        let value = secrets::require_credential(source)?;

        let targets: Vec<&str> = config["target"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect()
            })
            .unwrap_or_else(|| vec!["production", "preview", "development"]);

        set_env_var(vercel_key, &value, &targets)?;
        eprintln!("  ✓ {vercel_key}");
        synced += 1;
    }

    Ok(serde_json::json!({
        "ok": true,
        "synced": synced,
    }))
}

/// Get latest deployment status without waiting.
pub fn deployment_status() -> anyhow::Result<serde_json::Value> {
    let (client, token, _org_id, project_id) = vercel_client()?;

    let url = format!(
        "https://api.vercel.com/v6/deployments?projectId={project_id}&limit=1"
    );

    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .header("User-Agent", "logline-cli")
        .send()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("Vercel API error ({status}): {text}");
    }

    let body: serde_json::Value = resp.json()?;
    if let Some(deploy) = body["deployments"].as_array().and_then(|a| a.first()) {
        Ok(serde_json::json!({
            "deployment_id": deploy["uid"],
            "url": deploy["url"],
            "state": deploy["state"],
            "created_at": deploy["created"],
        }))
    } else {
        Ok(serde_json::json!({"state": "no_deployments"}))
    }
}
