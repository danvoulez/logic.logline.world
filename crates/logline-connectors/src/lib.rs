use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use logline_api::{
    BackendCapabilities, BackendConfig, BackendConnector, ConnectorFactory, DomainEvent,
    EventCursor, ExecutionResult, Intent, LoglineError, RunId, SecretStore,
};

pub struct EnvSecretStore;

impl SecretStore for EnvSecretStore {
    fn get(&self, secret_ref: &str) -> Result<String, LoglineError> {
        std::env::var(secret_ref)
            .map_err(|_| LoglineError::NotFound(format!("missing secret env var {secret_ref}")))
    }
}

pub struct HttpLikeConnector {
    id: String,
    base_url: String,
}

impl HttpLikeConnector {
    pub fn new(id: String, base_url: String) -> Self {
        Self { id, base_url }
    }
}

impl BackendConnector for HttpLikeConnector {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_streaming: true,
            supports_write: true,
            supports_history: true,
        }
    }

    fn health(&self) -> Result<(), LoglineError> {
        if self.base_url.is_empty() {
            return Err(LoglineError::Connection("base_url is empty".to_string()));
        }
        Ok(())
    }

    fn execute(&self, intent: &Intent) -> Result<ExecutionResult, LoglineError> {
        let run_id = format!("run-{}", now_ms());
        let mut output = BTreeMap::new();
        output.insert("backend".to_string(), self.id.clone());
        output.insert("intent_type".to_string(), intent.intent_type.clone());
        output.insert("target".to_string(), self.base_url.clone());

        Ok(ExecutionResult {
            run_id,
            status: "accepted".to_string(),
            output,
        })
    }

    fn stop(&self, _run_id: &RunId) -> Result<(), LoglineError> {
        Ok(())
    }

    fn events_since(&self, cursor: Option<&EventCursor>) -> Result<Vec<DomainEvent>, LoglineError> {
        let event = DomainEvent {
            cursor: format!("{}", now_ms()),
            ts_unix_ms: now_ms() as i64,
            kind: "heartbeat".to_string(),
            run_id: None,
            attributes: BTreeMap::from([
                ("backend".to_string(), self.id.clone()),
                (
                    "since".to_string(),
                    cursor.cloned().unwrap_or_else(|| "none".to_string()),
                ),
            ]),
        };
        Ok(vec![event])
    }
}

#[derive(Default)]
pub struct DefaultConnectorFactory;

impl ConnectorFactory for DefaultConnectorFactory {
    fn build(
        &self,
        cfg: &BackendConfig,
        _secrets: &dyn SecretStore,
    ) -> Result<Box<dyn BackendConnector>, LoglineError> {
        Ok(Box::new(HttpLikeConnector::new(
            cfg.backend_id.clone(),
            cfg.base_url.clone(),
        )))
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}
