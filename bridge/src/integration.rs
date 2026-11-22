//! Optional, read-only T adapter. Local engine commands never call this network boundary.
use crate::client::{BackendStatusView, TakoBackendClient};
use crate::{isolation, BridgeError, MissionSummary, ProjectView};
use serde::Serialize;
use std::sync::Mutex;
use tako_engine::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendAvailability {
    Disabled,
    NotChecked,
    Available,
    Unavailable,
}
#[derive(Debug, Serialize)]
pub struct IntegrationView {
    pub mode: &'static str,
    pub backend: Option<&'static str>,
    pub availability: BackendAvailability,
    pub read_only: bool,
    pub provider_execution: &'static str,
}
struct State {
    client: Option<TakoBackendClient>,
    availability: BackendAvailability,
    generation: u64,
}
pub struct OptionalTIntegration {
    state: Mutex<State>,
}
impl OptionalTIntegration {
    pub fn new(client: Option<TakoBackendClient>) -> Self {
        let availability = if client.is_some() {
            BackendAvailability::NotChecked
        } else {
            BackendAvailability::Disabled
        };
        Self {
            state: Mutex::new(State {
                client,
                availability,
                generation: 0,
            }),
        }
    }
    pub fn view(&self) -> IntegrationView {
        let state = self.state.lock().unwrap();
        IntegrationView {
            mode: if state.client.is_some() {
                "optional_backend"
            } else {
                "standalone"
            },
            backend: state.client.as_ref().map(|_| "t"),
            availability: state.availability,
            read_only: true,
            provider_execution: "disabled",
        }
    }
    pub fn configure(&self, enabled: bool) -> IntegrationView {
        // Credential/config I/O must never hold a lock needed by local commands.
        let existing = { self.state.lock().unwrap().client.clone() };
        let client = if enabled {
            Some(existing.unwrap_or_default())
        } else {
            None
        };
        let mut state = self.state.lock().unwrap();
        state.generation += 1;
        state.client = client;
        state.availability = if enabled {
            BackendAvailability::NotChecked
        } else {
            BackendAvailability::Disabled
        };
        drop(state);
        self.view()
    }
    fn snapshot(&self) -> Result<(TakoBackendClient, u64), BridgeError> {
        let state = self.state.lock().unwrap();
        state
            .client
            .clone()
            .map(|c| (c, state.generation))
            .ok_or_else(|| {
                BridgeError::new(
                    "integration_disabled",
                    "T integration is disabled. Local workflows remain available.",
                )
            })
    }
    pub fn read<T>(
        &self,
        action: impl FnOnce(&TakoBackendClient) -> Result<T, BridgeError>,
    ) -> Result<T, BridgeError> {
        let (client, generation) = self.snapshot()?;
        // Never hold a bridge/integration lock across network I/O.
        let result = action(&client);
        let state = self.state.lock().unwrap();
        if state.generation != generation {
            return Err(BridgeError::new(
                "integration_changed",
                "The integration changed; its previous response was discarded.",
            ));
        }
        result
    }
    pub fn status(&self) -> Result<BackendStatusView, BridgeError> {
        let (client, generation) = match self.snapshot() {
            Ok(s) => s,
            Err(_) => {
                return Ok(BackendStatusView {
                    online: false,
                    base_url: String::new(),
                    status: "DISABLED".into(),
                    runtime_status: "UNKNOWN".into(),
                    g12_state: "UNKNOWN".into(),
                    operational_mode: "STANDALONE".into(),
                    projects: vec![],
                    limits: None,
                    approvals: vec![],
                    workstations: vec![],
                    details: None,
                })
            }
        };
        let status = client.get_backend_status();
        let mut state = self.state.lock().unwrap();
        if state.generation != generation {
            return Err(BridgeError::new(
                "integration_changed",
                "The integration changed; its previous response was discarded.",
            ));
        }
        state.availability = if status.online {
            BackendAvailability::Available
        } else {
            BackendAvailability::Unavailable
        };
        Ok(status)
    }
    pub fn open_project(&self, project_id: &str) -> Result<ProjectView, BridgeError> {
        let view = self.read(|client| {
            let projects = client
                .list_projects()
                .map_err(|e| BridgeError::new("backend_unavailable", e))?;
            let p = projects
                .projects
                .into_iter()
                .find(|p| p.project_id == project_id)
                .ok_or_else(|| BridgeError::new("not_found", "Backend project was not found."))?;
            let goals = client
                .get_project_goals(&p.project_id)
                .map_err(|e| BridgeError::new("backend_unavailable", e))?;
            if goals.project_id != p.project_id
                || goals.goals.iter().any(|g| g.project_id != p.project_id)
            {
                return Err(BridgeError::new(
                    "evidence_invalid",
                    "Backend project and goal identities disagree.",
                ));
            }
            Ok(ProjectView {
                project: Project {
                    path: p.path,
                    branch: p.branch,
                    head: p.head,
                    dirty: p.dirty,
                },
                missions: goals
                    .goals
                    .into_iter()
                    .map(|g| MissionSummary {
                        id: g.id,
                        goal: g.goal,
                        state: None,
                        reported_backend_state: Some(g.state),
                        backend_project_id: Some(p.project_id.clone()),
                        created_at: 0,
                    })
                    .collect(),
                isolation: isolation(),
                source: "t_backend",
                backend_project_id: Some(p.project_id),
            })
        })?;
        // Backend registry data is never local filesystem/permission authority.
        // Returning a read-only backend view does not inspect the remote path.

        Ok(view)
    }
    pub fn is_protected_path(&self, path: &str) -> bool {
        legacy_protected_path(path)
    }
}

/// Retain preexisting conservative stabilization guards in every application mode.
/// These are mutation restrictions, not backend discovery or trust decisions.
pub fn legacy_protected_path(path: &str) -> bool {
    path.contains("projects/t-") || path.contains("t-master-sandboxes") || path.contains("era1")
}
pub fn legacy_protected_id(id: &str) -> bool {
    id.starts_with("PROG-") || id.starts_with("GOAL-") || id.starts_with("T-")
}
