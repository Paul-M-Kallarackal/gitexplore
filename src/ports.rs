use async_trait::async_trait;
use secrecy::SecretString;

use crate::{
    bookmarks::{Bookmark, BookmarkTarget, Category},
    discovery::{RepositoryCandidate, UserNeighborhood},
    exploration::{ExplorationResult, ExplorationSeed, ExplorationSnapshot},
    graph::{
        GitHubRateLimitLease, GitHubRateLimitStatus, GraphImport, RefreshLease,
        RefreshLeaseAttempt, RefreshLeaseState, SyncStatus, SyncSummary,
    },
    identity::{AuthSessionResult, CompletedBrowserLogin, GitHubConnection, PendingBrowserLogin},
    insights::{
        RepositoryContributorInsights, RepositoryContributorsSnapshot,
        UserCommitRepositoriesSnapshot, UserCommitRepositoryInsights,
    },
    shared::{AppError, AppResult},
};

#[derive(Debug, Clone)]
pub struct DeviceLoginStart {
    pub verification_uri: String,
    pub user_code: String,
    pub device_code: String,
}

#[derive(Debug, Clone)]
pub struct GitHubAuthConfig {
    pub client_id: SecretString,
    pub client_secret: Option<SecretString>,
    pub redirect_uri: Option<String>,
    pub scopes: Vec<String>,
}

#[async_trait]
pub trait IdentityRepository: Send + Sync {
    async fn get_connection(&self, user_id: &str) -> AppResult<Option<GitHubConnection>>;
    async fn save_connection(&self, user_id: &str, connection: GitHubConnection) -> AppResult<()>;
    async fn save_browser_connection(
        &self,
        preferred_user_id: &str,
        connection: GitHubConnection,
    ) -> AppResult<String>;
    async fn clear_connection(&self, user_id: &str) -> AppResult<()>;
    async fn save_pending_browser_login(
        &self,
        state_id: &str,
        pending: PendingBrowserLogin,
    ) -> AppResult<()>;
    async fn consume_pending_browser_login(
        &self,
        state_id: &str,
    ) -> AppResult<Option<PendingBrowserLogin>>;
    async fn create_session(&self, session_id: &str, user_id: &str) -> AppResult<()>;
    async fn get_user_id_for_session(&self, session_id: &str) -> AppResult<Option<String>>;
    async fn clear_session(&self, session_id: &str) -> AppResult<()>;
    async fn github_rate_limit(
        &self,
        github_user_id: i64,
    ) -> AppResult<Option<GitHubRateLimitStatus>>;
    async fn save_github_rate_limit(
        &self,
        github_user_id: i64,
        status: GitHubRateLimitStatus,
    ) -> AppResult<()>;
    async fn try_acquire_github_rate_limit_lease(
        &self,
        github_user_id: i64,
        token: &str,
        lease_seconds: i64,
    ) -> AppResult<Option<GitHubRateLimitLease>>;
    async fn renew_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
        lease_seconds: i64,
    ) -> AppResult<bool>;
    async fn release_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
    ) -> AppResult<bool>;
}

#[async_trait]
pub trait GitHubImportRepository: Send + Sync {
    async fn import_github_graph(
        &self,
        user_id: &str,
        import: GraphImport,
    ) -> AppResult<SyncSummary>;
    async fn try_acquire_refresh_lease(
        &self,
        entity_key: &str,
        token: &str,
        lease_seconds: i64,
    ) -> AppResult<RefreshLeaseAttempt> {
        Ok(RefreshLeaseAttempt::Acquired(RefreshLease {
            entity_key: entity_key.to_string(),
            token: token.to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(lease_seconds),
        }))
    }
    async fn renew_refresh_lease(
        &self,
        _lease: &RefreshLease,
        _lease_seconds: i64,
    ) -> AppResult<bool> {
        Ok(true)
    }
    async fn refresh_lease_state(&self, _entity_key: &str) -> AppResult<Option<RefreshLeaseState>> {
        Ok(None)
    }
    async fn complete_refresh_lease(
        &self,
        _lease: &RefreshLease,
        _outcome_json: Option<&str>,
    ) -> AppResult<bool> {
        Ok(true)
    }
    async fn fail_refresh_lease(&self, _lease: &RefreshLease, _error: &str) -> AppResult<bool> {
        Ok(true)
    }
    async fn import_github_graph_under_lease(
        &self,
        user_id: &str,
        import: GraphImport,
        lease: &RefreshLease,
        canonical_login: &str,
    ) -> AppResult<SyncSummary> {
        let summary = self.import_github_graph(user_id, import).await?;
        let outcome = crate::graph::UserRefreshOutcome {
            canonical_login: canonical_login.to_string(),
            summary: summary.clone(),
        };
        let outcome_json = serde_json::to_string(&outcome)?;
        if !self
            .complete_refresh_lease(lease, Some(&outcome_json))
            .await?
        {
            return Err(AppError::External(
                "refresh lease was lost before completion".to_string(),
            ));
        }
        Ok(summary)
    }
    async fn resolve_bookmark_target(&self, target: &BookmarkTarget) -> AppResult<()>;
}

#[async_trait]
pub trait SyncStateRepository: Send + Sync {
    async fn sync_status(&self, user_id: &str) -> AppResult<SyncStatus>;
    async fn set_sync_status(&self, user_id: &str, status: SyncStatus) -> AppResult<()>;
}

#[async_trait]
pub trait CategoryRepository: Send + Sync {
    async fn create_category(&self, user_id: &str, category: Category) -> AppResult<()>;
    async fn list_categories(&self, user_id: &str) -> AppResult<Vec<Category>>;
}

#[async_trait]
pub trait BookmarkRepository: Send + Sync {
    async fn add_bookmark(&self, user_id: &str, bookmark: Bookmark) -> AppResult<Bookmark>;
    async fn list_bookmarks(&self, user_id: &str) -> AppResult<Vec<Bookmark>>;
}

#[async_trait]
pub trait ExplorationRepository: Send + Sync {
    async fn explore(&self, user_id: &str, seed: ExplorationSeed) -> AppResult<ExplorationResult>;
    async fn list_exploration_snapshots(
        &self,
        user_id: &str,
    ) -> AppResult<Vec<ExplorationSnapshot>>;
}

#[async_trait]
pub trait DiscoveryRepository: Send + Sync {
    async fn user_neighborhood(&self, user_id: &str, login: &str) -> AppResult<UserNeighborhood>;
    async fn discover_repositories(
        &self,
        user_id: &str,
        login: &str,
        limit: usize,
    ) -> AppResult<Vec<RepositoryCandidate>>;
}

#[async_trait]
pub trait InsightRepository: Send + Sync {
    async fn repository_contributors(
        &self,
        full_name: &str,
    ) -> AppResult<Option<RepositoryContributorInsights>>;
    async fn user_commit_repositories(
        &self,
        login: &str,
    ) -> AppResult<Option<UserCommitRepositoryInsights>>;
    async fn begin_repository_contributor_refresh(&self, full_name: &str) -> AppResult<bool>;
    async fn begin_user_commit_repository_refresh(&self, login: &str) -> AppResult<bool>;
    async fn save_repository_contributors(
        &self,
        insights: RepositoryContributorInsights,
    ) -> AppResult<()>;
    async fn save_user_commit_repositories(
        &self,
        insights: UserCommitRepositoryInsights,
    ) -> AppResult<()>;
    async fn fail_repository_contributor_refresh(
        &self,
        full_name: &str,
        error: &str,
    ) -> AppResult<()>;
    async fn fail_user_commit_repository_refresh(&self, login: &str, error: &str) -> AppResult<()>;
}

#[async_trait]
pub trait GitHubClientPort: Send + Sync {
    async fn start_device_flow(&self, config: &GitHubAuthConfig) -> AppResult<DeviceLoginStart>;
    async fn finish_device_flow(
        &self,
        config: &GitHubAuthConfig,
        device_code: &str,
    ) -> AppResult<GitHubConnection>;
    async fn exchange_browser_code(
        &self,
        config: &GitHubAuthConfig,
        code: &str,
    ) -> AppResult<GitHubConnection>;
    async fn fetch_graph(&self, connection: &GitHubConnection) -> AppResult<GraphImport>;
    async fn fetch_user_graph(
        &self,
        connection: &GitHubConnection,
        login: &str,
    ) -> AppResult<GraphImport>;
    async fn fetch_core_rate_limit(
        &self,
        connection: &GitHubConnection,
    ) -> AppResult<GitHubRateLimitStatus>;
    async fn fetch_repository_contributors(
        &self,
        _connection: &GitHubConnection,
        _full_name: &str,
    ) -> AppResult<RepositoryContributorsSnapshot> {
        Err(AppError::Unsupported(
            "repository contributor insights are not supported by this GitHub client".to_string(),
        ))
    }
    async fn fetch_user_commit_repositories(
        &self,
        _connection: &GitHubConnection,
        _login: &str,
    ) -> AppResult<UserCommitRepositoriesSnapshot> {
        Err(AppError::Unsupported(
            "user commit insights are not supported by this GitHub client".to_string(),
        ))
    }
    fn browser_oauth_url(&self, config: &GitHubAuthConfig, state: &str) -> AppResult<String>;
}

#[async_trait]
pub trait IdentityService: Send + Sync {
    async fn start_device_login(&self, user_id: &str) -> AppResult<AuthSessionResult>;
    async fn complete_device_login(
        &self,
        user_id: &str,
        device_code: &str,
    ) -> AppResult<AuthSessionResult>;
    async fn start_browser_login(
        &self,
        user_id: &str,
        redirect_to: Option<String>,
        browser_nonce: &str,
    ) -> AppResult<String>;
    async fn complete_browser_login(
        &self,
        state_id: &str,
        code: &str,
        browser_nonce: &str,
    ) -> AppResult<CompletedBrowserLogin>;
    async fn connection_status(
        &self,
        user_id: &str,
    ) -> AppResult<crate::identity::ConnectionStatus>;
    async fn create_session(&self, user_id: &str) -> AppResult<String>;
    async fn resolve_session(&self, session_id: &str) -> AppResult<Option<String>>;
    async fn clear_session(&self, session_id: &str) -> AppResult<()>;
    async fn logout(&self, user_id: &str) -> AppResult<()>;
}

#[async_trait]
pub trait GitHubSyncService: Send + Sync {
    async fn run_sync(&self, user_id: &str) -> AppResult<SyncSummary>;
    async fn status(&self, user_id: &str) -> AppResult<SyncStatus>;
    async fn rate_limit(&self, user_id: &str) -> AppResult<GitHubRateLimitStatus>;
}

#[async_trait]
pub trait BookmarkService: Send + Sync {
    async fn create_category(
        &self,
        user_id: &str,
        name: &str,
        description: Option<String>,
    ) -> AppResult<()>;
    async fn list_categories(&self, user_id: &str) -> AppResult<Vec<Category>>;
    async fn add_bookmark(
        &self,
        user_id: &str,
        target: BookmarkTarget,
        categories: Vec<String>,
        note: Option<String>,
    ) -> AppResult<Bookmark>;
    async fn list_bookmarks(&self, user_id: &str) -> AppResult<Vec<Bookmark>>;
}

#[async_trait]
pub trait ExplorationService: Send + Sync {
    async fn explore(&self, user_id: &str, seed: ExplorationSeed) -> AppResult<ExplorationResult>;
    async fn snapshots(&self, user_id: &str) -> AppResult<Vec<ExplorationSnapshot>>;
}

#[async_trait]
pub trait DiscoveryService: Send + Sync {
    async fn user_neighborhood(&self, user_id: &str, login: &str) -> AppResult<UserNeighborhood>;
    async fn discover_repositories(
        &self,
        user_id: &str,
        login: &str,
        limit: usize,
    ) -> AppResult<Vec<RepositoryCandidate>>;
    async fn expand_user(&self, user_id: &str, login: &str) -> AppResult<UserNeighborhood>;
    async fn expand_user_with_reserve(
        &self,
        user_id: &str,
        login: &str,
        request_reserve: usize,
    ) -> AppResult<UserNeighborhood>;
}

#[async_trait]
pub trait InsightService: Send + Sync {
    async fn repository_contributors(
        &self,
        user_id: &str,
        full_name: &str,
        limit: usize,
    ) -> AppResult<RepositoryContributorInsights>;
    async fn user_commit_repositories(
        &self,
        user_id: &str,
        login: &str,
        limit: usize,
    ) -> AppResult<UserCommitRepositoryInsights>;
}
