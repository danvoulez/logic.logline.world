use std::sync::RwLock;

use logline_api::{
    BackendConfig, BackendConnector, BackendId, ConnectorFactory, DomainEvent, EventCursor,
    ExecutionResult, Intent, LoglineError, ProfileId, RunId, RuntimeEngine, RuntimeStatus,
    SecretStore,
};
use logline_connectors::{DefaultConnectorFactory, EnvSecretStore};
use logline_core::{ConnectionCatalog, validate_catalog};

struct RuntimeState {
    active_profile: ProfileId,
    active_backend: BackendId,
    running_jobs: usize,
}

pub struct LoglineRuntime {
    catalog: ConnectionCatalog,
    connectors: std::collections::BTreeMap<BackendId, Box<dyn BackendConnector>>,
    state: RwLock<RuntimeState>,
}

impl LoglineRuntime {
    pub fn from_catalog(catalog: ConnectionCatalog) -> Result<Self, LoglineError> {
        validate_catalog(&catalog)?;

        let secrets = EnvSecretStore;
        Self::from_catalog_with_factory(catalog, &DefaultConnectorFactory, &secrets)
    }

    pub fn from_catalog_with_factory(
        catalog: ConnectionCatalog,
        factory: &dyn ConnectorFactory,
        secrets: &dyn SecretStore,
    ) -> Result<Self, LoglineError> {
        validate_catalog(&catalog)?;

        let mut connectors = std::collections::BTreeMap::new();
        for (id, cfg) in &catalog.backends {
            connectors.insert(id.clone(), build_connector(factory, cfg, secrets)?);
        }

        let first_profile = catalog
            .profiles
            .keys()
            .next()
            .ok_or_else(|| LoglineError::Validation("no profiles configured".to_string()))?
            .clone();

        let active_backend = catalog
            .profiles
            .get(&first_profile)
            .map(|p| p.backend_id.clone())
            .ok_or_else(|| {
                LoglineError::Validation("active profile missing backend".to_string())
            })?;

        Ok(Self {
            catalog,
            connectors,
            state: RwLock::new(RuntimeState {
                active_profile: first_profile,
                active_backend,
                running_jobs: 0,
            }),
        })
    }
}

impl RuntimeEngine for LoglineRuntime {
    fn status(&self) -> Result<RuntimeStatus, LoglineError> {
        let guard = self
            .state
            .read()
            .map_err(|_| LoglineError::Internal("runtime state poisoned".to_string()))?;
        Ok(RuntimeStatus {
            active_profile: guard.active_profile.clone(),
            active_backend: guard.active_backend.clone(),
            running_jobs: guard.running_jobs,
            queue_depth: 0,
        })
    }

    fn run_intent(&self, intent: Intent) -> Result<ExecutionResult, LoglineError> {
        let backend_id = {
            let guard = self
                .state
                .read()
                .map_err(|_| LoglineError::Internal("runtime state poisoned".to_string()))?;
            guard.active_backend.clone()
        };

        let connector = self
            .connectors
            .get(&backend_id)
            .ok_or_else(|| LoglineError::NotFound(format!("backend {backend_id} not loaded")))?;
        connector.execute(&intent)
    }

    fn stop_run(&self, run_id: RunId) -> Result<(), LoglineError> {
        let backend_id = {
            let guard = self
                .state
                .read()
                .map_err(|_| LoglineError::Internal("runtime state poisoned".to_string()))?;
            guard.active_backend.clone()
        };
        let connector = self
            .connectors
            .get(&backend_id)
            .ok_or_else(|| LoglineError::NotFound(format!("backend {backend_id} not loaded")))?;
        connector.stop(&run_id)
    }

    fn events_since(&self, cursor: Option<EventCursor>) -> Result<Vec<DomainEvent>, LoglineError> {
        let backend_id = {
            let guard = self
                .state
                .read()
                .map_err(|_| LoglineError::Internal("runtime state poisoned".to_string()))?;
            guard.active_backend.clone()
        };
        let connector = self
            .connectors
            .get(&backend_id)
            .ok_or_else(|| LoglineError::NotFound(format!("backend {backend_id} not loaded")))?;
        connector.events_since(cursor.as_ref())
    }

    fn test_backend(&self, backend_id: BackendId) -> Result<(), LoglineError> {
        let connector = self
            .connectors
            .get(&backend_id)
            .ok_or_else(|| LoglineError::NotFound(format!("backend {backend_id} not loaded")))?;
        connector.health()
    }

    fn select_profile(&self, profile_id: ProfileId) -> Result<(), LoglineError> {
        let profile = self
            .catalog
            .profiles
            .get(&profile_id)
            .ok_or_else(|| LoglineError::NotFound(format!("profile {profile_id} not found")))?;

        if !self.connectors.contains_key(&profile.backend_id) {
            return Err(LoglineError::NotFound(format!(
                "backend {} not loaded",
                profile.backend_id
            )));
        }

        let mut guard = self
            .state
            .write()
            .map_err(|_| LoglineError::Internal("runtime state poisoned".to_string()))?;
        guard.active_profile = profile_id;
        guard.active_backend = profile.backend_id.clone();
        Ok(())
    }
}

fn build_connector(
    factory: &dyn ConnectorFactory,
    cfg: &BackendConfig,
    secrets: &dyn SecretStore,
) -> Result<Box<dyn BackendConnector>, LoglineError> {
    factory.build(cfg, secrets)
}
