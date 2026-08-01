use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::graph::{CacheStatus, GraphNodeRef, RefreshJobStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExplorationSeed {
    User { login: String },
    Repository { full_name: String },
    Category { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationSnapshot {
    pub id: String,
    pub seed: ExplorationSeed,
    pub discovered_people: Vec<String>,
    pub discovered_repositories: Vec<String>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationResult {
    pub seed: ExplorationSeed,
    pub related_people: Vec<String>,
    pub related_repositories: Vec<String>,
    pub saved_snapshot: ExplorationSnapshot,
    pub cache_status: CacheStatus,
    pub last_fetched_at: Option<DateTime<Utc>>,
    pub refresh_job_status: Option<RefreshJobStatus>,
    pub overload_message: Option<String>,
}

impl From<GraphNodeRef> for ExplorationSeed {
    fn from(value: GraphNodeRef) -> Self {
        match value {
            GraphNodeRef::User { login } => Self::User { login },
            GraphNodeRef::Repository { full_name } => Self::Repository { full_name },
        }
    }
}
