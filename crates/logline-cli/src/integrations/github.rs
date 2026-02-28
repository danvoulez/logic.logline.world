use std::process::{Command, Stdio};

use anyhow::{bail, ensure};

use crate::commands::secrets;

/// Push current branch to origin using GitHub token from Keychain.
/// Token is injected via git extraheader — never persisted in git config.
pub fn git_push() -> anyhow::Result<serde_json::Value> {
    let token = secrets::require_credential_or_env("github_token", "LOGLINE_GITHUB_TOKEN")?;

    let branch = current_branch()?;
    eprintln!("Pushing {branch} to origin...");

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    let header_val = format!("AUTHORIZATION: basic {encoded}");

    let output = Command::new("git")
        .args([
            "-c",
            &format!("http.https://github.com/.extraheader={header_val}"),
        ])
        .args(["push", "origin", "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;

    ensure!(output.status.success(), "git push failed");

    let sha = head_sha()?;
    let remote = remote_url()?;

    Ok(serde_json::json!({
        "ok": true,
        "branch": branch,
        "commit_sha": sha,
        "remote_url": remote,
    }))
}

/// Create a pull request via GitHub REST API.
pub fn create_pr(title: &str, body: &str, base: &str) -> anyhow::Result<serde_json::Value> {
    let token = secrets::require_credential_or_env("github_token", "LOGLINE_GITHUB_TOKEN")?;
    let (owner, repo) = parse_remote()?;
    let head = current_branch()?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client
        .post(format!("https://api.github.com/repos/{owner}/{repo}/pulls"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "logline-cli")
        .json(&serde_json::json!({
            "title": title,
            "body": body,
            "head": head,
            "base": base,
        }))
        .send()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("GitHub PR creation failed ({status}): {text}");
    }

    let pr: serde_json::Value = resp.json()?;
    Ok(serde_json::json!({
        "ok": true,
        "pr_number": pr["number"],
        "pr_url": pr["html_url"],
        "head": head,
        "base": base,
    }))
}

/// Create a GitHub release via REST API.
pub fn create_release(tag: &str, notes: Option<&str>) -> anyhow::Result<serde_json::Value> {
    let token = secrets::require_credential_or_env("github_token", "LOGLINE_GITHUB_TOKEN")?;
    let (owner, repo) = parse_remote()?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client
        .post(format!(
            "https://api.github.com/repos/{owner}/{repo}/releases"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "logline-cli")
        .json(&serde_json::json!({
            "tag_name": tag,
            "name": tag,
            "body": notes.unwrap_or(""),
            "draft": false,
            "prerelease": false,
        }))
        .send()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("GitHub release creation failed ({status}): {text}");
    }

    let release: serde_json::Value = resp.json()?;
    Ok(serde_json::json!({
        "ok": true,
        "tag": tag,
        "release_url": release["html_url"],
        "release_id": release["id"],
    }))
}

// ─── Git helpers ────────────────────────────────────────────────────────────

fn current_branch() -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;
    ensure!(output.status.success(), "Failed to get current branch");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn head_sha() -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    ensure!(output.status.success(), "Failed to get HEAD SHA");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn remote_url() -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()?;
    ensure!(output.status.success(), "Failed to get remote URL");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_remote() -> anyhow::Result<(String, String)> {
    let url = remote_url()?;
    // Handle both HTTPS and SSH formats
    let cleaned = url
        .trim_end_matches(".git")
        .replace("git@github.com:", "https://github.com/");

    let parts: Vec<&str> = cleaned.trim_end_matches('/').rsplit('/').collect();
    ensure!(parts.len() >= 2, "Cannot parse owner/repo from remote URL: {url}");
    Ok((parts[1].to_string(), parts[0].to_string()))
}
