use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use logline_api::{AuthMode, BackendAuth, BackendConfig, LoglineError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePolicy {
    pub max_concurrent_runs: usize,
    pub default_queue_capacity: usize,
    pub stop_grace_seconds: u64,
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            max_concurrent_runs: 4,
            default_queue_capacity: 200,
            stop_grace_seconds: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub backend_id: String,
    pub readonly: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionCatalog {
    pub profiles: BTreeMap<String, Profile>,
    pub backends: BTreeMap<String, BackendConfig>,
}

pub fn validate_catalog(catalog: &ConnectionCatalog) -> Result<(), LoglineError> {
    for (id, profile) in &catalog.profiles {
        if !catalog.backends.contains_key(&profile.backend_id) {
            return Err(LoglineError::Validation(format!(
                "profile {id} points to missing backend {}",
                profile.backend_id
            )));
        }
    }
    Ok(())
}

pub fn demo_catalog() -> ConnectionCatalog {
    let backend_id = "local-main".to_string();
    let backend = BackendConfig {
        backend_id: backend_id.clone(),
        base_url: "http://127.0.0.1:8787".to_string(),
        auth: logline_api::BackendAuth {
            mode: logline_api::AuthMode::ApiKey,
            secret_ref: "LOGLINE_LOCAL_API_KEY".to_string(),
        },
        connect_timeout_ms: 2_000,
        request_timeout_ms: 10_000,
        extra_headers: BTreeMap::new(),
    };

    let profile = Profile {
        id: "local".to_string(),
        backend_id: backend_id.clone(),
        readonly: false,
    };

    ConnectionCatalog {
        profiles: BTreeMap::from([(profile.id.clone(), profile)]),
        backends: BTreeMap::from([(backend_id, backend)]),
    }
}

#[derive(Debug, Deserialize)]
struct RawConnections {
    profiles: BTreeMap<String, RawProfile>,
    backends: BTreeMap<String, RawBackend>,
}

#[derive(Debug, Deserialize)]
struct RawProfile {
    backend: String,
    #[serde(default)]
    readonly: bool,
}

#[derive(Debug, Deserialize)]
struct RawBackend {
    base_url: String,
    auth_mode: AuthMode,
    secret_ref: String,
    connect_timeout_ms: u64,
    request_timeout_ms: u64,
    #[serde(default)]
    extra_headers: BTreeMap<String, String>,
}

pub fn default_config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("logline")
    } else {
        PathBuf::from(".logline")
    }
}

pub fn load_catalog_from_dir(dir: &Path) -> Result<ConnectionCatalog, LoglineError> {
    let path = dir.join("connections.toml");
    load_catalog_from_file(&path)
}

pub fn load_catalog_from_file(path: &Path) -> Result<ConnectionCatalog, LoglineError> {
    let content = fs::read_to_string(path)
        .map_err(|e| LoglineError::NotFound(format!("failed to read {}: {e}", path.display())))?;
    let raw: RawConnections = toml::from_str(&content).map_err(|e| {
        LoglineError::Validation(format!("invalid TOML in {}: {e}", path.display()))
    })?;

    let profiles = raw
        .profiles
        .into_iter()
        .map(|(id, p)| {
            (
                id.clone(),
                Profile {
                    id,
                    backend_id: p.backend,
                    readonly: p.readonly,
                },
            )
        })
        .collect();

    let backends = raw
        .backends
        .into_iter()
        .map(|(id, b)| {
            (
                id.clone(),
                BackendConfig {
                    backend_id: id,
                    base_url: b.base_url,
                    auth: BackendAuth {
                        mode: b.auth_mode,
                        secret_ref: b.secret_ref,
                    },
                    connect_timeout_ms: b.connect_timeout_ms,
                    request_timeout_ms: b.request_timeout_ms,
                    extra_headers: b.extra_headers,
                },
            )
        })
        .collect();

    let catalog = ConnectionCatalog { profiles, backends };
    validate_catalog(&catalog)?;
    Ok(catalog)
}

pub fn write_default_config_files(dir: &Path) -> Result<(), LoglineError> {
    fs::create_dir_all(dir)
        .map_err(|e| LoglineError::Internal(format!("failed to create {}: {e}", dir.display())))?;

    let files: [(&str, &str); 3] = [
        (
            "connections.toml",
            include_str!("../../../docs/logline-cli/examples/connections.toml.example"),
        ),
        (
            "runtime.toml",
            include_str!("../../../docs/logline-cli/examples/runtime.toml.example"),
        ),
        (
            "ui.toml",
            include_str!("../../../docs/logline-cli/examples/ui.toml.example"),
        ),
    ];

    for (name, body) in files {
        let path = dir.join(name);
        if !path.exists() {
            fs::write(&path, body).map_err(|e| {
                LoglineError::Internal(format!("failed to write {}: {e}", path.display()))
            })?;
        }
    }

    Ok(())
}
