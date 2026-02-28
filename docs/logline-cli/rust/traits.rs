//! v1 trait contracts for logline runtime, connectors, and control surfaces.

use std::collections::BTreeMap;
use std::time::Duration;

pub type ProfileId = String;
pub type BackendId = String;
pub type RunId = String;
pub type EventCursor = String;

#[derive(Debug, Clone)]
pub enum AuthMode {
    ApiKey,
    Bearer,
    Mtls,
}

#[derive(Debug, Clone)]
pub struct BackendAuth {
    pub mode: AuthMode,
    pub secret_ref: String,
}

#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub backend_id: BackendId,
    pub base_url: String,
    pub auth: BackendAuth,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub extra_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct BackendCapabilities {
    pub supports_streaming: bool,
    pub supports_write: bool,
    pub supports_history: bool,
}

#[derive(Debug, Clone)]
pub struct Intent {
    pub intent_type: String,
    pub payload: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub run_id: RunId,
    pub status: String,
    pub output: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub active_profile: ProfileId,
    pub active_backend: BackendId,
    pub running_jobs: usize,
    pub queue_depth: usize,
}

#[derive(Debug, Clone)]
pub struct DomainEvent {
    pub cursor: EventCursor,
    pub ts_unix_ms: i64,
    pub kind: String,
    pub run_id: Option<RunId>,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug)]
pub enum LoglineError {
    Validation(String),
    Auth(String),
    Connection(String),
    Conflict(String),
    NotFound(String),
    Internal(String),
}

pub trait SecretStore: Send + Sync {
    fn get(&self, secret_ref: &str) -> Result<String, LoglineError>;
}

pub trait BackendConnector: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> BackendCapabilities;
    fn health(&self) -> Result<(), LoglineError>;
    fn execute(&self, intent: &Intent) -> Result<ExecutionResult, LoglineError>;
    fn stop(&self, run_id: &RunId) -> Result<(), LoglineError>;
    fn events_since(&self, cursor: Option<&EventCursor>) -> Result<Vec<DomainEvent>, LoglineError>;
}

pub trait ConnectorFactory: Send + Sync {
    fn build(
        &self,
        cfg: &BackendConfig,
        secrets: &dyn SecretStore,
    ) -> Result<Box<dyn BackendConnector>, LoglineError>;
}

pub trait RuntimeEngine: Send + Sync {
    fn status(&self) -> Result<RuntimeStatus, LoglineError>;
    fn run_intent(&self, intent: Intent) -> Result<ExecutionResult, LoglineError>;
    fn stop_run(&self, run_id: RunId) -> Result<(), LoglineError>;
    fn events_since(&self, cursor: Option<EventCursor>) -> Result<Vec<DomainEvent>, LoglineError>;
    fn test_backend(&self, backend_id: BackendId) -> Result<(), LoglineError>;
    fn select_profile(&self, profile_id: ProfileId) -> Result<(), LoglineError>;
}
