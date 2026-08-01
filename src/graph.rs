use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitHubUserNode {
    pub github_id: i64,
    pub login: String,
    pub name: Option<String>,
    pub url: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub followers_count: Option<u64>,
    #[serde(default)]
    pub following_count: Option<u64>,
    #[serde(default)]
    pub public_repositories_count: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitHubRepositoryNode {
    pub github_id: i64,
    pub owner_login: String,
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub stargazer_count: u64,
    #[serde(default)]
    pub fork_count: u64,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub pushed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub is_fork: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphImport {
    pub viewer: Option<GitHubUserNode>,
    pub followers: Vec<GitHubUserNode>,
    pub following: Vec<GitHubUserNode>,
    pub starred_repositories: Vec<GitHubRepositoryNode>,
    pub repositories: Vec<GitHubRepositoryNode>,
    #[serde(default)]
    pub coverage: GraphImportCoverage,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphImportCoverage {
    pub followers_complete: bool,
    pub following_complete: bool,
    pub starred_repositories_complete: bool,
    pub repositories_complete: bool,
}

impl GraphImportCoverage {
    pub fn is_complete(self) -> bool {
        self.followers_complete
            && self.following_complete
            && self.starred_repositories_complete
            && self.repositories_complete
    }

    pub const fn incomplete() -> Self {
        Self {
            followers_complete: false,
            following_complete: false,
            starred_repositories_complete: false,
            repositories_complete: false,
        }
    }
}

impl Default for GraphImportCoverage {
    fn default() -> Self {
        Self {
            followers_complete: true,
            following_complete: true,
            starred_repositories_complete: true,
            repositories_complete: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheMetadata {
    pub last_fetched_at: Option<DateTime<Utc>>,
    pub stale_at: Option<DateTime<Utc>>,
    pub refresh_started_at: Option<DateTime<Utc>>,
    pub last_refresh_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheStatus {
    Fresh,
    Stale,
    Refreshing,
    RefreshFailed,
}

impl std::fmt::Display for CacheStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fresh => write!(f, "fresh"),
            Self::Stale => write!(f, "stale"),
            Self::Refreshing => write!(f, "refreshing"),
            Self::RefreshFailed => write!(f, "refresh_failed"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RefreshJobStatus {
    Queued,
    Running,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncState {
    NeverSynced,
    SyncInProgress,
    SyncSucceeded,
    SyncFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub state: SyncState,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            state: SyncState::NeverSynced,
            last_synced_at: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSummary {
    pub followers: usize,
    pub following: usize,
    pub starred_repositories: usize,
    pub repositories: usize,
    pub synced_at: DateTime<Utc>,
    pub coverage: GraphImportCoverage,
}

#[derive(Debug, Clone)]
pub struct RefreshLease {
    pub entity_key: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshLeaseStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub struct RefreshLeaseState {
    pub status: RefreshLeaseStatus,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub expired: bool,
    pub outcome_json: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RefreshLeaseAttempt {
    Acquired(RefreshLease),
    Busy(RefreshLeaseState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRefreshOutcome {
    pub canonical_login: String,
    pub summary: SyncSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubRateLimitStatus {
    pub limit: usize,
    pub used: usize,
    pub remaining: usize,
    pub reset_at: DateTime<Utc>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct GitHubRateLimitLease {
    pub github_user_id: i64,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GraphNodeRef {
    User { login: String },
    Repository { full_name: String },
}
