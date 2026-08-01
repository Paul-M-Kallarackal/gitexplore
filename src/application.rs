use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, Weak},
    time::Duration as StdDuration,
};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use secrecy::ExposeSecret;
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    task::JoinSet,
    time::{self, Instant},
};
use uuid::Uuid;

use crate::{
    bookmarks::{Bookmark, BookmarkTarget, Category},
    discovery::{
        ExplorationActivity, ExplorationDirection, MAX_RECENT_PEOPLE, MAX_SAVED_TRAIL_ENTRIES,
        RepositoryCandidate, UserNeighborhood,
    },
    exploration::{ExplorationResult, ExplorationSeed, ExplorationSnapshot},
    graph::{
        CacheStatus, DiscoveryWarmupJob, DiscoveryWarmupStatus, GitHubRateLimitLease,
        GitHubRateLimitStatus, RefreshLease, RefreshLeaseAttempt, RefreshLeaseStatus, SyncState,
        SyncStatus, SyncSummary, UserRefreshOutcome,
    },
    identity::{
        AuthSessionResult, CompletedBrowserLogin, ConnectionStatus, GitHubConnection,
        PendingBrowserLogin,
    },
    insights::{RepositoryContributorInsights, UserCommitRepositoryInsights},
    onboarding::{
        CURRENT_ONBOARDING_VERSION, OnboardingProgress, OnboardingRecord, OnboardingStatus,
    },
    ports::{
        BookmarkRepository, BookmarkService, CategoryRepository, DiscoveryRepository,
        DiscoveryService, ExplorationRepository, ExplorationService, GitHubAuthConfig,
        GitHubClientPort, GitHubImportRepository, GitHubSyncService, IdentityRepository,
        IdentityService, InsightRepository, InsightService, OnboardingService, SyncStateRepository,
    },
    shared::{AppError, AppResult, GITHUB_CORE_REST_MINIMUM_RESERVE, ensure},
};

#[derive(Clone)]
pub struct AppServices {
    pub identity: Arc<dyn IdentityService>,
    pub sync: Arc<dyn GitHubSyncService>,
    pub bookmarks: Arc<dyn BookmarkService>,
    pub exploration: Arc<dyn ExplorationService>,
    pub discovery: Arc<dyn DiscoveryService>,
    pub insights: Arc<dyn InsightService>,
    pub onboarding: Arc<dyn OnboardingService>,
}

#[derive(Clone)]
pub struct AppServiceRepositories {
    pub identity: Arc<dyn IdentityRepository>,
    pub imports: Arc<dyn GitHubImportRepository>,
    pub sync_state: Arc<dyn SyncStateRepository>,
    pub categories: Arc<dyn CategoryRepository>,
    pub bookmarks: Arc<dyn BookmarkRepository>,
    pub exploration: Arc<dyn ExplorationRepository>,
    pub discovery: Arc<dyn DiscoveryRepository>,
    pub insights: Arc<dyn InsightRepository>,
}

impl AppServices {
    pub fn new(
        repositories: AppServiceRepositories,
        github: Arc<dyn GitHubClientPort>,
        github_auth: GitHubAuthConfig,
    ) -> Self {
        let AppServiceRepositories {
            identity: identity_repo,
            imports: import_repo,
            sync_state: sync_state_repo,
            categories: category_repo,
            bookmarks: bookmark_repo,
            exploration: exploration_repo,
            discovery: discovery_repo,
            insights: insight_repo,
        } = repositories;
        let refreshes = RefreshCoordinator::default();
        let rate_budgets = GitHubRateBudgetCoordinator {
            identity_repo: identity_repo.clone(),
            github: github.clone(),
        };
        let identity = Arc::new(DefaultIdentityService {
            identity_repo: identity_repo.clone(),
            github: github.clone(),
            github_auth: github_auth.clone(),
        });
        let sync = Arc::new(DefaultGitHubSyncService {
            identity_repo: identity_repo.clone(),
            import_repo: import_repo.clone(),
            sync_state_repo: sync_state_repo.clone(),
            github: github.clone(),
            refreshes: refreshes.clone(),
            rate_budgets: rate_budgets.clone(),
        });
        let bookmarks = Arc::new(DefaultBookmarkService {
            import_repo: import_repo.clone(),
            category_repo,
            bookmark_repo: bookmark_repo.clone(),
        });
        let exploration = Arc::new(DefaultExplorationService { exploration_repo });
        let discovery = Arc::new(DefaultDiscoveryService {
            identity_repo: identity_repo.clone(),
            import_repo: import_repo.clone(),
            sync_state_repo: sync_state_repo.clone(),
            discovery_repo: discovery_repo.clone(),
            github: github.clone(),
            refreshes,
            rate_budgets: rate_budgets.clone(),
            warmup_scheduler: DiscoveryWarmupScheduler::default(),
        });
        let insights = Arc::new(DefaultInsightService {
            identity_repo,
            import_repo,
            insight_repo,
            github,
            rate_budgets,
            cold_refreshes: InsightColdRefreshCoordinator::default(),
        });
        let onboarding = Arc::new(DefaultOnboardingService {
            sync_state_repo,
            bookmark_repo,
            discovery_repo,
        });

        Self {
            identity,
            sync,
            bookmarks,
            exploration,
            discovery,
            insights,
            onboarding,
        }
    }
}

pub struct DefaultIdentityService {
    identity_repo: Arc<dyn IdentityRepository>,
    github: Arc<dyn GitHubClientPort>,
    github_auth: GitHubAuthConfig,
}

const BROWSER_LOGIN_TTL_MINUTES: i64 = 10;

#[async_trait]
impl IdentityService for DefaultIdentityService {
    async fn start_device_login(&self, _user_id: &str) -> AppResult<AuthSessionResult> {
        ensure(
            !self.github_auth.client_id.expose_secret().is_empty(),
            "github client id is required",
        )?;
        let start = self.github.start_device_flow(&self.github_auth).await?;
        Ok(AuthSessionResult {
            verification_uri: Some(start.verification_uri),
            user_code: Some(start.user_code),
            connected_account: None,
        })
    }

    async fn complete_device_login(
        &self,
        user_id: &str,
        device_code: &str,
    ) -> AppResult<AuthSessionResult> {
        let connection = self
            .github
            .finish_device_flow(&self.github_auth, device_code)
            .await?;
        let connected_account = connection.account.clone();
        self.identity_repo
            .save_connection(user_id, connection.clone())
            .await?;
        self.persist_post_oauth_rate_status(&connection).await;
        Ok(AuthSessionResult {
            verification_uri: None,
            user_code: None,
            connected_account: Some(connected_account),
        })
    }

    async fn start_browser_login(
        &self,
        user_id: &str,
        redirect_to: Option<String>,
        browser_nonce: &str,
    ) -> AppResult<String> {
        ensure(
            !browser_nonce.trim().is_empty(),
            "browser nonce is required",
        )?;
        let state_id = Uuid::new_v4().to_string();
        let url = self
            .github
            .browser_oauth_url(&self.github_auth, &state_id)?;
        let created_at = Utc::now();
        self.identity_repo
            .save_pending_browser_login(
                &state_id,
                PendingBrowserLogin {
                    user_id: user_id.to_string(),
                    redirect_to,
                    browser_nonce: browser_nonce.to_string(),
                    created_at,
                    expires_at: created_at + Duration::minutes(BROWSER_LOGIN_TTL_MINUTES),
                },
            )
            .await?;
        Ok(url)
    }

    async fn complete_browser_login(
        &self,
        state_id: &str,
        code: &str,
        browser_nonce: &str,
    ) -> AppResult<CompletedBrowserLogin> {
        let pending = self
            .identity_repo
            .consume_pending_browser_login(state_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation("invalid or already-used OAuth state".to_string())
            })?;
        ensure(
            pending.browser_nonce == browser_nonce,
            "OAuth state does not belong to this browser",
        )?;
        let state_age = Utc::now().signed_duration_since(pending.created_at);
        ensure(
            state_age >= Duration::minutes(-1) && Utc::now() <= pending.expires_at,
            "OAuth state expired",
        )?;
        let connection = self
            .github
            .exchange_browser_code(&self.github_auth, code)
            .await?;
        let connected_account = connection.account.clone();
        let canonical_user_id = self
            .identity_repo
            .save_browser_connection(&pending.user_id, connection.clone())
            .await?;
        self.persist_post_oauth_rate_status(&connection).await;
        let session_id = self.create_session(&canonical_user_id).await?;
        Ok(CompletedBrowserLogin {
            result: AuthSessionResult {
                verification_uri: None,
                user_code: None,
                connected_account: Some(connected_account),
            },
            session_id,
            redirect_to: pending.redirect_to,
        })
    }

    async fn connection_status(&self, user_id: &str) -> AppResult<ConnectionStatus> {
        let connection = self.identity_repo.get_connection(user_id).await?;
        Ok(ConnectionStatus {
            authenticated: true,
            app_user_id: Some(user_id.to_string()),
            connected: connection.is_some(),
            account: connection.map(|item| item.account),
        })
    }

    async fn create_session(&self, user_id: &str) -> AppResult<String> {
        let session_id = Uuid::new_v4().to_string();
        self.identity_repo
            .create_session(&session_id, user_id)
            .await?;
        Ok(session_id)
    }

    async fn resolve_session(&self, session_id: &str) -> AppResult<Option<String>> {
        self.identity_repo.get_user_id_for_session(session_id).await
    }

    async fn clear_session(&self, session_id: &str) -> AppResult<()> {
        self.identity_repo.clear_session(session_id).await
    }

    async fn logout(&self, user_id: &str) -> AppResult<()> {
        self.identity_repo.clear_connection(user_id).await
    }
}

impl DefaultIdentityService {
    async fn persist_post_oauth_rate_status(&self, connection: &GitHubConnection) {
        let status = match self.github.fetch_core_rate_limit(connection).await {
            Ok(status) => status,
            Err(error) => {
                tracing::warn!(%error, "could not observe the post-OAuth GitHub rate budget");
                return;
            }
        };
        if let Err(error) = self
            .identity_repo
            .save_github_rate_limit(connection.account.github_user_id, status)
            .await
        {
            tracing::warn!(%error, "could not persist the post-OAuth GitHub rate budget");
        }
    }
}

pub const GITHUB_CORE_REST_RESERVE: usize = GITHUB_CORE_REST_MINIMUM_RESERVE;
const GITHUB_GRAPH_EXPANSION_MAX_REQUESTS: usize = 13;
const GITHUB_REPOSITORY_CONTRIBUTORS_MAX_REQUESTS: usize = 1;
const GITHUB_USER_EVENTS_MAX_REQUESTS: usize = 3;
const DISCOVERY_WARMUP_TOTAL_USER_LIMIT: usize = 10_000;
const DISCOVERY_WARMUP_LOCAL_WORKER_LIMIT: usize = 4;
const DISCOVERY_WARMUP_RUNNABLE_SCAN_LIMIT: usize = 32;
const DISCOVERY_WARMUP_BATCH_PAUSE_MILLISECONDS: u64 = 25;
const DISCOVERY_WARMUP_PEER_DEFER_MILLISECONDS: u64 = 1_000;
const RATE_BUDGET_LEASE_SECONDS: i64 = 120;
const RATE_BUDGET_HEARTBEAT_SECONDS: u64 = 30;
const RATE_BUDGET_WAIT_SECONDS: u64 = 300;
const RATE_LIMIT_DISPLAY_CACHE_MINUTES: i64 = 1;

#[derive(Clone)]
struct GitHubRateBudgetCoordinator {
    identity_repo: Arc<dyn IdentityRepository>,
    github: Arc<dyn GitHubClientPort>,
}

impl GitHubRateBudgetCoordinator {
    async fn observe(&self, connection: &GitHubConnection) -> AppResult<GitHubRateLimitStatus> {
        let github_user_id = connection.account.github_user_id;
        let cached = self.identity_repo.github_rate_limit(github_user_id).await?;
        let cutoff = Utc::now() - Duration::minutes(RATE_LIMIT_DISPLAY_CACHE_MINUTES);
        if let Some(status) = cached.as_ref().filter(|status| status.checked_at >= cutoff) {
            return Ok(status.clone());
        }

        let token = Uuid::new_v4().to_string();
        let Some(lease) = self
            .identity_repo
            .try_acquire_github_rate_limit_lease(github_user_id, &token, RATE_BUDGET_LEASE_SECONDS)
            .await?
        else {
            if let Some(cached) = cached {
                return Ok(cached);
            }
            let lease = self.acquire_lease(github_user_id, token).await?;
            return self.probe_and_release(connection, lease).await;
        };

        if let Some(status) = self
            .identity_repo
            .github_rate_limit(github_user_id)
            .await?
            .filter(|status| status.checked_at >= cutoff)
        {
            let _ = self
                .identity_repo
                .release_github_rate_limit_lease(&lease)
                .await;
            return Ok(status);
        }
        self.probe_and_release(connection, lease).await
    }

    async fn begin(
        &self,
        connection: &GitHubConnection,
        operation: &str,
        requested_cost: usize,
    ) -> AppResult<GitHubRateBudgetGuard> {
        self.begin_with_reserve(
            connection,
            operation,
            requested_cost,
            GITHUB_CORE_REST_RESERVE,
        )
        .await
    }

    async fn begin_with_reserve(
        &self,
        connection: &GitHubConnection,
        operation: &str,
        requested_cost: usize,
        reserve: usize,
    ) -> AppResult<GitHubRateBudgetGuard> {
        ensure(
            reserve >= GITHUB_CORE_REST_RESERVE,
            format!("GitHub request reserve cannot be lower than {GITHUB_CORE_REST_RESERVE}"),
        )?;
        let github_user_id = connection.account.github_user_id;
        let lease = self
            .acquire_lease(github_user_id, Uuid::new_v4().to_string())
            .await?;
        let mut guard = GitHubRateBudgetGuard {
            coordinator: self.clone(),
            connection: connection.clone(),
            lease: Some(lease),
        };
        let status = match self.probe(connection).await {
            Ok(status) => status,
            Err(error) => {
                guard.release().await;
                return Err(error);
            }
        };
        if status.remaining.saturating_sub(requested_cost) < reserve {
            let error = AppError::RateBudgetReserved {
                operation: operation.to_string(),
                remaining: status.remaining,
                reserve,
                requested_cost,
                reset_at: status.reset_at,
            };
            guard.release().await;
            return Err(error);
        }
        Ok(guard)
    }

    async fn acquire_lease(
        &self,
        github_user_id: i64,
        token: String,
    ) -> AppResult<GitHubRateLimitLease> {
        let deadline = Instant::now() + StdDuration::from_secs(RATE_BUDGET_WAIT_SECONDS);
        loop {
            if let Some(lease) = self
                .identity_repo
                .try_acquire_github_rate_limit_lease(
                    github_user_id,
                    &token,
                    RATE_BUDGET_LEASE_SECONDS,
                )
                .await?
            {
                return Ok(lease);
            }
            if Instant::now() >= deadline {
                return Err(AppError::External(
                    "timed out waiting for this GitHub account's REST budget lease".to_string(),
                ));
            }
            time::sleep(StdDuration::from_millis(100)).await;
        }
    }

    async fn probe(&self, connection: &GitHubConnection) -> AppResult<GitHubRateLimitStatus> {
        let status = self.github.fetch_core_rate_limit(connection).await?;
        self.identity_repo
            .save_github_rate_limit(connection.account.github_user_id, status.clone())
            .await?;
        Ok(status)
    }

    async fn probe_and_release(
        &self,
        connection: &GitHubConnection,
        lease: GitHubRateLimitLease,
    ) -> AppResult<GitHubRateLimitStatus> {
        let result = self.probe(connection).await;
        let release = self
            .identity_repo
            .release_github_rate_limit_lease(&lease)
            .await;
        match (result, release) {
            (Ok(status), Ok(_)) => Ok(status),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}

struct GitHubRateBudgetGuard {
    coordinator: GitHubRateBudgetCoordinator,
    connection: GitHubConnection,
    lease: Option<GitHubRateLimitLease>,
}

impl GitHubRateBudgetGuard {
    fn lease(&self) -> &GitHubRateLimitLease {
        self.lease.as_ref().expect("rate-budget guard is armed")
    }

    async fn renew(&self) -> AppResult<bool> {
        self.coordinator
            .identity_repo
            .renew_github_rate_limit_lease(self.lease(), RATE_BUDGET_LEASE_SECONDS)
            .await
    }

    async fn finish(mut self) {
        if let Err(error) = self.coordinator.probe(&self.connection).await {
            tracing::warn!(%error, "could not persist the post-operation GitHub rate budget");
        }
        self.release().await;
    }

    async fn release(&mut self) {
        if let Some(lease) = self.lease.take()
            && let Err(error) = self
                .coordinator
                .identity_repo
                .release_github_rate_limit_lease(&lease)
                .await
        {
            tracing::warn!(%error, "could not release the GitHub rate-budget lease");
        }
    }
}

impl Drop for GitHubRateBudgetGuard {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        let identity_repo = self.coordinator.identity_repo.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = identity_repo.release_github_rate_limit_lease(&lease).await;
            });
        }
    }
}

pub struct DefaultGitHubSyncService {
    identity_repo: Arc<dyn IdentityRepository>,
    import_repo: Arc<dyn GitHubImportRepository>,
    sync_state_repo: Arc<dyn SyncStateRepository>,
    github: Arc<dyn GitHubClientPort>,
    refreshes: RefreshCoordinator,
    rate_budgets: GitHubRateBudgetCoordinator,
}

#[async_trait]
impl GitHubSyncService for DefaultGitHubSyncService {
    async fn run_sync(&self, user_id: &str) -> AppResult<SyncSummary> {
        let connection = self
            .identity_repo
            .get_connection(user_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation("user is not authenticated with GitHub".to_string())
            })?;
        let entity_key = refresh_key(&connection.account.login);
        let (flight, is_leader) = self.refreshes.join(&entity_key).await;
        if !is_leader {
            self.sync_state_repo
                .set_sync_status(
                    user_id,
                    SyncStatus {
                        state: SyncState::SyncInProgress,
                        last_synced_at: None,
                        last_error: None,
                    },
                )
                .await?;
            let result = RefreshCoordinator::wait(&flight).await;
            match &result {
                Ok(summary) => {
                    self.sync_state_repo
                        .set_sync_status(
                            user_id,
                            SyncStatus {
                                state: SyncState::SyncSucceeded,
                                last_synced_at: Some(summary.synced_at),
                                last_error: None,
                            },
                        )
                        .await?;
                }
                Err(error) => {
                    self.sync_state_repo
                        .set_sync_status(
                            user_id,
                            SyncStatus {
                                state: SyncState::SyncFailed,
                                last_synced_at: None,
                                last_error: Some(error.to_string()),
                            },
                        )
                        .await?;
                }
            }
            return result;
        }

        let refresh_guard = RefreshLeaderGuard::new(self.refreshes.clone(), entity_key, flight);
        let result = self.run_sync_leader(user_id, connection).await;
        refresh_guard.finish(&result).await;
        result
    }

    async fn status(&self, user_id: &str) -> AppResult<SyncStatus> {
        self.sync_state_repo.sync_status(user_id).await
    }

    async fn rate_limit(&self, user_id: &str) -> AppResult<GitHubRateLimitStatus> {
        let connection = self
            .identity_repo
            .get_connection(user_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation("user is not authenticated with GitHub".to_string())
            })?;
        self.rate_budgets.observe(&connection).await
    }
}

impl DefaultGitHubSyncService {
    async fn run_sync_leader(
        &self,
        user_id: &str,
        connection: GitHubConnection,
    ) -> AppResult<SyncSummary> {
        self.sync_state_repo
            .set_sync_status(
                user_id,
                SyncStatus {
                    state: SyncState::SyncInProgress,
                    last_synced_at: None,
                    last_error: None,
                },
            )
            .await?;
        match self.sync_graph(user_id, connection).await {
            Ok(summary) => {
                self.sync_state_repo
                    .set_sync_status(
                        user_id,
                        SyncStatus {
                            state: SyncState::SyncSucceeded,
                            last_synced_at: Some(summary.synced_at),
                            last_error: None,
                        },
                    )
                    .await?;
                Ok(summary)
            }
            Err(error) => {
                self.sync_state_repo
                    .set_sync_status(
                        user_id,
                        SyncStatus {
                            state: SyncState::SyncFailed,
                            last_synced_at: None,
                            last_error: Some(error.to_string()),
                        },
                    )
                    .await?;
                Err(error)
            }
        }
    }

    async fn sync_graph(
        &self,
        user_id: &str,
        connection: GitHubConnection,
    ) -> AppResult<SyncSummary> {
        let entity_key = refresh_key(&connection.account.login);
        match enter_durable_refresh(&self.import_repo, &entity_key).await? {
            DurableRefreshRole::Completed(Some(outcome)) => {
                Ok(serde_json::from_str::<UserRefreshOutcome>(&outcome)?.summary)
            }
            DurableRefreshRole::Completed(None) => Err(AppError::Storage(
                "completed user refresh is missing its durable outcome".to_string(),
            )),
            DurableRefreshRole::Leader(lease) => {
                let mut guard = DurableRefreshGuard::new(self.import_repo.clone(), lease);
                let budget_guard = match begin_rate_budget_under_refresh_lease(
                    &self.rate_budgets,
                    self.import_repo.clone(),
                    guard.lease(),
                    &connection,
                    "GitHub graph synchronization",
                    GITHUB_GRAPH_EXPANSION_MAX_REQUESTS,
                    GITHUB_CORE_REST_RESERVE,
                )
                .await
                {
                    Ok(budget_guard) => budget_guard,
                    Err(error) => {
                        guard.fail(&error).await;
                        return Err(error);
                    }
                };
                let graph_result = fetch_with_refresh_heartbeat(
                    self.import_repo.clone(),
                    guard.lease(),
                    fetch_with_rate_budget_heartbeat(
                        &budget_guard,
                        self.github.fetch_graph(&connection),
                    ),
                )
                .await;
                budget_guard.finish().await;
                let graph = match graph_result {
                    Ok(graph) => graph,
                    Err(error) => {
                        guard.fail(&error).await;
                        return Err(error);
                    }
                };
                let canonical_login = graph
                    .viewer
                    .as_ref()
                    .map(|viewer| viewer.login.clone())
                    .unwrap_or_else(|| connection.account.login.clone());
                match self
                    .import_repo
                    .import_github_graph_under_lease(
                        user_id,
                        graph,
                        guard.lease(),
                        &canonical_login,
                    )
                    .await
                {
                    Ok(summary) => {
                        guard.disarm();
                        Ok(summary)
                    }
                    Err(error) => {
                        guard.fail(&error).await;
                        Err(error)
                    }
                }
            }
        }
    }
}

pub struct DefaultBookmarkService {
    import_repo: Arc<dyn GitHubImportRepository>,
    category_repo: Arc<dyn CategoryRepository>,
    bookmark_repo: Arc<dyn BookmarkRepository>,
}

#[async_trait]
impl BookmarkService for DefaultBookmarkService {
    async fn create_category(
        &self,
        user_id: &str,
        name: &str,
        description: Option<String>,
    ) -> AppResult<()> {
        ensure(!name.trim().is_empty(), "category name cannot be empty")?;
        self.category_repo
            .create_category(
                user_id,
                Category {
                    name: name.trim().to_string(),
                    description,
                },
            )
            .await
    }

    async fn list_categories(&self, user_id: &str) -> AppResult<Vec<Category>> {
        self.category_repo.list_categories(user_id).await
    }

    async fn add_bookmark(
        &self,
        user_id: &str,
        target: BookmarkTarget,
        categories: Vec<String>,
        note: Option<String>,
    ) -> AppResult<Bookmark> {
        self.import_repo.resolve_bookmark_target(&target).await?;
        let bookmark = Bookmark {
            id: Uuid::new_v4().to_string(),
            target,
            categories,
            note,
            created_at: Utc::now(),
        };
        self.bookmark_repo.add_bookmark(user_id, bookmark).await
    }

    async fn list_bookmarks(&self, user_id: &str) -> AppResult<Vec<Bookmark>> {
        self.bookmark_repo.list_bookmarks(user_id).await
    }
}

pub struct DefaultExplorationService {
    exploration_repo: Arc<dyn ExplorationRepository>,
}

#[async_trait]
impl ExplorationService for DefaultExplorationService {
    async fn explore(&self, user_id: &str, seed: ExplorationSeed) -> AppResult<ExplorationResult> {
        self.exploration_repo.explore(user_id, seed).await
    }

    async fn snapshots(&self, user_id: &str) -> AppResult<Vec<ExplorationSnapshot>> {
        self.exploration_repo
            .list_exploration_snapshots(user_id)
            .await
    }
}

pub struct DefaultOnboardingService {
    sync_state_repo: Arc<dyn SyncStateRepository>,
    bookmark_repo: Arc<dyn BookmarkRepository>,
    discovery_repo: Arc<dyn DiscoveryRepository>,
}

impl DefaultOnboardingService {
    async fn current_record(&self, user_id: &str) -> AppResult<Option<OnboardingRecord>> {
        Ok(self
            .sync_state_repo
            .onboarding_record(user_id)
            .await?
            .filter(|record| record.version == CURRENT_ONBOARDING_VERSION))
    }

    async fn progress_for_record(
        &self,
        user_id: &str,
        record: OnboardingRecord,
    ) -> AppResult<OnboardingProgress> {
        let (activity, bookmarks, warmup) = tokio::try_join!(
            self.discovery_repo
                .exploration_activity(user_id, MAX_RECENT_PEOPLE),
            self.bookmark_repo.list_bookmarks(user_id),
            self.sync_state_repo.discovery_warmup(user_id),
        )?;

        let (opened_trailhead, followed_connection, saved_repository) =
            if record.status == OnboardingStatus::Completed {
                (true, true, true)
            } else if let Some(started_at) = record.started_at {
                let recent_visits = activity
                    .recent_people
                    .iter()
                    .filter(|person| person.last_viewed_at >= started_at)
                    .collect::<Vec<_>>();
                (
                    !recent_visits.is_empty(),
                    recent_visits.iter().any(|person| person.trail.len() >= 2),
                    bookmarks.iter().any(|bookmark| {
                        bookmark.created_at >= started_at
                            && matches!(bookmark.target, BookmarkTarget::GitHubRepository { .. })
                    }),
                )
            } else {
                (false, false, false)
            };

        Ok(OnboardingProgress {
            version: record.version,
            status: record.status,
            started_at: record.started_at,
            completed_at: record.completed_at,
            dismissed_at: record.dismissed_at,
            opened_trailhead,
            followed_connection,
            saved_repository,
            mapping_started: warmup.is_some(),
        })
    }
}

#[async_trait]
impl OnboardingService for DefaultOnboardingService {
    async fn progress(&self, user_id: &str) -> AppResult<OnboardingProgress> {
        let Some(record) = self.current_record(user_id).await? else {
            return Ok(OnboardingProgress::not_started());
        };
        self.progress_for_record(user_id, record).await
    }

    async fn begin(&self, user_id: &str) -> AppResult<OnboardingProgress> {
        if let Some(record) = self.current_record(user_id).await? {
            return self.progress_for_record(user_id, record).await;
        }
        let record = OnboardingRecord::in_progress(Utc::now());
        self.sync_state_repo
            .save_onboarding_record(user_id, record.clone())
            .await?;
        self.progress_for_record(user_id, record).await
    }

    async fn dismiss(&self, user_id: &str) -> AppResult<OnboardingProgress> {
        let now = Utc::now();
        let mut record = self
            .current_record(user_id)
            .await?
            .unwrap_or_else(|| OnboardingRecord::in_progress(now));
        if record.status != OnboardingStatus::Completed {
            record.status = OnboardingStatus::Dismissed;
            record.started_at.get_or_insert(now);
            record.completed_at = None;
            record.dismissed_at = Some(now);
            self.sync_state_repo
                .save_onboarding_record(user_id, record.clone())
                .await?;
        }
        self.progress_for_record(user_id, record).await
    }

    async fn restart(&self, user_id: &str) -> AppResult<OnboardingProgress> {
        let record = OnboardingRecord::in_progress(Utc::now());
        self.sync_state_repo
            .save_onboarding_record(user_id, record.clone())
            .await?;
        self.progress_for_record(user_id, record).await
    }

    async fn complete(&self, user_id: &str) -> AppResult<OnboardingProgress> {
        let Some(mut record) = self.current_record(user_id).await? else {
            return Err(AppError::Validation(
                "onboarding must be started before it can be completed".to_string(),
            ));
        };
        if record.status == OnboardingStatus::Completed {
            return self.progress_for_record(user_id, record).await;
        }
        ensure(
            record.status == OnboardingStatus::InProgress,
            "onboarding must be active before it can be completed",
        )?;
        let progress = self.progress_for_record(user_id, record.clone()).await?;
        ensure(
            progress.required_steps_complete(),
            "open a trailhead, follow a connection, and save a repository before completing onboarding",
        )?;
        let now = Utc::now();
        record.status = OnboardingStatus::Completed;
        record.completed_at = Some(now);
        record.dismissed_at = None;
        self.sync_state_repo
            .save_onboarding_record(user_id, record.clone())
            .await?;
        self.progress_for_record(user_id, record).await
    }
}

#[derive(Clone)]
pub struct DefaultDiscoveryService {
    identity_repo: Arc<dyn IdentityRepository>,
    import_repo: Arc<dyn GitHubImportRepository>,
    sync_state_repo: Arc<dyn SyncStateRepository>,
    discovery_repo: Arc<dyn DiscoveryRepository>,
    github: Arc<dyn GitHubClientPort>,
    refreshes: RefreshCoordinator,
    rate_budgets: GitHubRateBudgetCoordinator,
    warmup_scheduler: DiscoveryWarmupScheduler,
}

#[derive(Clone, Default)]
struct DiscoveryWarmupScheduler {
    state: Arc<AsyncMutex<DiscoveryWarmupSchedulerState>>,
}

#[derive(Default)]
struct DiscoveryWarmupSchedulerState {
    running: bool,
    generation: u64,
    active_user_ids: HashSet<String>,
    deferred_until: HashMap<String, Instant>,
}

#[async_trait]
impl DiscoveryService for DefaultDiscoveryService {
    async fn user_neighborhood(&self, user_id: &str, login: &str) -> AppResult<UserNeighborhood> {
        let login = normalize_github_login(login)?;
        self.discovery_repo.user_neighborhood(user_id, &login).await
    }

    async fn discover_repositories(
        &self,
        user_id: &str,
        login: &str,
        limit: usize,
    ) -> AppResult<Vec<RepositoryCandidate>> {
        let login = normalize_github_login(login)?;
        ensure(
            (1..=100).contains(&limit),
            "repository discovery limit must be between 1 and 100",
        )?;
        self.discovery_repo
            .discover_repositories(user_id, &login, limit)
            .await
    }

    async fn exploration_activity(
        &self,
        user_id: &str,
        limit: usize,
    ) -> AppResult<ExplorationActivity> {
        ensure(
            (1..=MAX_RECENT_PEOPLE).contains(&limit),
            format!("recent people limit must be between 1 and {MAX_RECENT_PEOPLE}"),
        )?;
        self.discovery_repo
            .exploration_activity(user_id, limit)
            .await
    }

    async fn record_person_visit(
        &self,
        user_id: &str,
        login: &str,
        trail: Vec<String>,
        direction: ExplorationDirection,
    ) -> AppResult<ExplorationActivity> {
        let login = normalize_github_login(login)?;
        ensure(!trail.is_empty(), "exploration trail cannot be empty")?;
        ensure(
            trail.len() <= MAX_SAVED_TRAIL_ENTRIES,
            format!("exploration trail cannot contain more than {MAX_SAVED_TRAIL_ENTRIES} people"),
        )?;
        let trail = trail
            .into_iter()
            .map(|entry| normalize_github_login(&entry))
            .collect::<AppResult<Vec<_>>>()?;
        ensure_distinct_exploration_trail(&trail)?;
        ensure(
            trail
                .last()
                .is_some_and(|current| current.eq_ignore_ascii_case(&login)),
            "exploration trail must end at the visited person",
        )?;
        self.discovery_repo
            .record_person_visit(user_id, &login, trail, direction)
            .await
    }

    async fn set_recent_person_visible(
        &self,
        user_id: &str,
        login: &str,
        visible: bool,
    ) -> AppResult<ExplorationActivity> {
        let login = normalize_github_login(login)?;
        self.discovery_repo
            .set_recent_person_visible(user_id, &login, visible)
            .await
    }

    async fn expand_user(&self, user_id: &str, login: &str) -> AppResult<UserNeighborhood> {
        self.expand_user_with_reserve(user_id, login, GITHUB_CORE_REST_RESERVE)
            .await
    }

    async fn expand_user_with_reserve(
        &self,
        user_id: &str,
        login: &str,
        request_reserve: usize,
    ) -> AppResult<UserNeighborhood> {
        ensure(
            request_reserve >= GITHUB_CORE_REST_RESERVE,
            format!("GitHub request reserve cannot be lower than {GITHUB_CORE_REST_RESERVE}"),
        )?;
        let login = normalize_github_login(login)?;
        let entity_key = refresh_key(&login);
        let (flight, is_leader) = self.refreshes.join(&entity_key).await;
        if !is_leader {
            RefreshCoordinator::wait(&flight).await?;
            return self.discovery_repo.user_neighborhood(user_id, &login).await;
        }

        let refresh_guard = RefreshLeaderGuard::new(self.refreshes.clone(), entity_key, flight);
        let result = async {
            let connection = self
                .identity_repo
                .get_connection(user_id)
                .await?
                .ok_or_else(|| {
                    AppError::Validation("user is not authenticated with GitHub".to_string())
                })?;
            let (summary, canonical_login) =
                match enter_durable_refresh(&self.import_repo, &refresh_key(&login)).await? {
                    DurableRefreshRole::Completed(Some(outcome)) => {
                        let outcome = serde_json::from_str::<UserRefreshOutcome>(&outcome)?;
                        (outcome.summary, outcome.canonical_login)
                    }
                    DurableRefreshRole::Completed(None) => {
                        return Err(AppError::Storage(
                            "completed user refresh is missing its durable outcome".to_string(),
                        ));
                    }
                    DurableRefreshRole::Leader(lease) => {
                        let mut guard = DurableRefreshGuard::new(self.import_repo.clone(), lease);
                        let budget_guard = match begin_rate_budget_under_refresh_lease(
                            &self.rate_budgets,
                            self.import_repo.clone(),
                            guard.lease(),
                            &connection,
                            "GitHub user expansion",
                            GITHUB_GRAPH_EXPANSION_MAX_REQUESTS,
                            request_reserve,
                        )
                        .await
                        {
                            Ok(budget_guard) => budget_guard,
                            Err(error) => {
                                guard.fail(&error).await;
                                return Err(error);
                            }
                        };
                        let graph_result = fetch_with_refresh_heartbeat(
                            self.import_repo.clone(),
                            guard.lease(),
                            fetch_with_rate_budget_heartbeat(
                                &budget_guard,
                                self.github.fetch_user_graph(&connection, &login),
                            ),
                        )
                        .await;
                        budget_guard.finish().await;
                        let graph = match graph_result {
                            Ok(graph) => graph,
                            Err(error) => {
                                guard.fail(&error).await;
                                return Err(error);
                            }
                        };
                        let canonical_login = graph
                            .viewer
                            .as_ref()
                            .map(|viewer| viewer.login.clone())
                            .unwrap_or_else(|| login.clone());
                        match self
                            .import_repo
                            .import_github_graph_under_lease(
                                user_id,
                                graph,
                                guard.lease(),
                                &canonical_login,
                            )
                            .await
                        {
                            Ok(summary) => {
                                guard.disarm();
                                (summary, canonical_login)
                            }
                            Err(error) => {
                                guard.fail(&error).await;
                                return Err(error);
                            }
                        }
                    }
                };
            let neighborhood = self
                .discovery_repo
                .user_neighborhood(user_id, &canonical_login)
                .await?;
            Ok::<_, AppError>((summary, neighborhood))
        }
        .await;
        let shared_result = result
            .as_ref()
            .map(|(summary, _)| summary.clone())
            .map_err(|error| AppError::External(error.to_string()));
        refresh_guard.finish(&shared_result).await;
        result.map(|(_, neighborhood)| neighborhood)
    }

    async fn start_warmup(&self, user_id: &str) -> AppResult<DiscoveryWarmupJob> {
        let connection = self.github_connection(user_id).await?;
        let seed_login = normalize_github_login(&connection.account.login)?;
        let now = Utc::now();
        if let Some(existing) = self.sync_state_repo.discovery_warmup(user_id).await?
            && existing.status == DiscoveryWarmupStatus::ReserveProtected
            && existing.reset_at.is_some_and(|reset_at| reset_at <= now)
        {
            let mut resumed = existing;
            resumed.status = DiscoveryWarmupStatus::Queued;
            resumed.current_login = None;
            resumed.remaining_requests = None;
            resumed.reset_at = None;
            resumed.updated_at = now;
            resumed.completed_at = None;
            resumed.last_error = None;
            let warmup = self
                .sync_state_repo
                .resume_discovery_warmup_after_reset(user_id, resumed)
                .await?;
            if warmup.status.is_runnable() {
                self.kick_warmup_scheduler().await;
            }
            return Ok(warmup);
        }
        let warmup = self
            .sync_state_repo
            .start_discovery_warmup(
                user_id,
                DiscoveryWarmupJob {
                    id: Uuid::new_v4().to_string(),
                    seed_login: seed_login.clone(),
                    status: DiscoveryWarmupStatus::Queued,
                    current_login: None,
                    expanded_logins: Vec::new(),
                    frontier: vec![seed_login],
                    frontier_truncated: false,
                    remaining_requests: None,
                    reserve_requests: GITHUB_CORE_REST_RESERVE,
                    reset_at: None,
                    started_at: now,
                    updated_at: now,
                    completed_at: None,
                    last_error: None,
                },
            )
            .await?;
        if warmup.status.is_runnable() {
            self.kick_warmup_scheduler().await;
        }
        Ok(warmup)
    }

    async fn warmup_status(&self, user_id: &str) -> AppResult<Option<DiscoveryWarmupJob>> {
        let warmup = self.sync_state_repo.discovery_warmup(user_id).await?;
        if warmup
            .as_ref()
            .is_some_and(|warmup| warmup.status.is_runnable())
        {
            self.kick_warmup_scheduler().await;
        }
        Ok(warmup)
    }

    async fn resume_warmups(&self) -> AppResult<()> {
        self.kick_warmup_scheduler().await;
        Ok(())
    }
}

enum DiscoveryWarmupBatchResult {
    Continue,
    Stop,
    PeerActive,
}

impl DefaultDiscoveryService {
    async fn github_connection(&self, user_id: &str) -> AppResult<GitHubConnection> {
        self.identity_repo
            .get_connection(user_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation("user is not authenticated with GitHub".to_string())
            })
    }

    async fn kick_warmup_scheduler(&self) {
        let should_spawn = {
            let mut scheduler = self.warmup_scheduler.state.lock().await;
            scheduler.generation = scheduler.generation.wrapping_add(1);
            if scheduler.running {
                false
            } else {
                scheduler.running = true;
                true
            }
        };
        if should_spawn {
            let service = self.clone();
            tokio::spawn(async move {
                service.run_warmup_scheduler().await;
            });
        }
    }

    async fn run_warmup_scheduler(&self) {
        let mut workers = JoinSet::new();
        loop {
            let scan_generation = self.warmup_scheduler.state.lock().await.generation;
            if workers.len() < DISCOVERY_WARMUP_LOCAL_WORKER_LIMIT {
                match self
                    .sync_state_repo
                    .runnable_discovery_warmups(DISCOVERY_WARMUP_RUNNABLE_SCAN_LIMIT)
                    .await
                {
                    Ok(candidates) => {
                        let mut scheduler = self.warmup_scheduler.state.lock().await;
                        let now = Instant::now();
                        scheduler
                            .deferred_until
                            .retain(|_, retry_at| *retry_at > now);
                        let available = DISCOVERY_WARMUP_LOCAL_WORKER_LIMIT
                            .saturating_sub(scheduler.active_user_ids.len());
                        let to_schedule = candidates
                            .into_iter()
                            .filter(|user_id| {
                                !scheduler.active_user_ids.contains(user_id)
                                    && !scheduler.deferred_until.contains_key(user_id)
                            })
                            .take(available)
                            .collect::<Vec<_>>();
                        for user_id in to_schedule {
                            scheduler.active_user_ids.insert(user_id.clone());
                            let service = self.clone();
                            workers.spawn(async move {
                                let result = service.run_warmup_batch(&user_id).await;
                                (user_id, result)
                            });
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "could not scan runnable discovery warmups");
                        if workers.is_empty() {
                            self.warmup_scheduler.state.lock().await.running = false;
                            return;
                        }
                    }
                }
            }

            if workers.is_empty() {
                let retry_at = {
                    let mut scheduler = self.warmup_scheduler.state.lock().await;
                    if scheduler.generation != scan_generation {
                        continue;
                    }
                    let retry_at = scheduler.deferred_until.values().copied().min();
                    if retry_at.is_none() {
                        scheduler.running = false;
                    }
                    retry_at
                };
                if let Some(retry_at) = retry_at {
                    time::sleep_until(retry_at).await;
                    continue;
                }
                return;
            }

            match workers.join_next().await {
                Some(Ok((user_id, result))) => {
                    let peer_active = matches!(result, Ok(DiscoveryWarmupBatchResult::PeerActive));
                    let mut scheduler = self.warmup_scheduler.state.lock().await;
                    scheduler.active_user_ids.remove(&user_id);
                    if peer_active {
                        scheduler.deferred_until.insert(
                            user_id.clone(),
                            Instant::now()
                                + StdDuration::from_millis(
                                    DISCOVERY_WARMUP_PEER_DEFER_MILLISECONDS,
                                ),
                        );
                    }
                    drop(scheduler);
                    if let Err(error) = result {
                        tracing::warn!(app_user_id = %user_id, %error, "discovery warmup batch failed");
                    }
                }
                Some(Err(error)) => {
                    tracing::error!(%error, "discovery warmup worker task failed");
                    workers.abort_all();
                    while workers.join_next().await.is_some() {}
                    self.warmup_scheduler
                        .state
                        .lock()
                        .await
                        .active_user_ids
                        .clear();
                }
                None => {}
            }
            time::sleep(StdDuration::from_millis(
                DISCOVERY_WARMUP_BATCH_PAUSE_MILLISECONDS,
            ))
            .await;
        }
    }

    async fn run_warmup_batch(&self, user_id: &str) -> AppResult<DiscoveryWarmupBatchResult> {
        let Some(warmup) = self.sync_state_repo.discovery_warmup(user_id).await? else {
            return Ok(DiscoveryWarmupBatchResult::Stop);
        };
        if !warmup.status.is_runnable() {
            return Ok(DiscoveryWarmupBatchResult::Stop);
        }

        let entity_key = warmup_refresh_key(user_id);
        let lease = match enter_durable_refresh(&self.import_repo, &entity_key).await? {
            DurableRefreshRole::Leader(lease) => lease,
            DurableRefreshRole::Completed(_) => {
                return Ok(DiscoveryWarmupBatchResult::PeerActive);
            }
        };
        let mut guard = DurableRefreshGuard::new(self.import_repo.clone(), lease);
        let Some(mut warmup) = self.sync_state_repo.discovery_warmup(user_id).await? else {
            self.complete_warmup_lease(&mut guard).await?;
            return Ok(DiscoveryWarmupBatchResult::Stop);
        };
        if !warmup.status.is_runnable() {
            self.complete_warmup_lease(&mut guard).await?;
            return Ok(DiscoveryWarmupBatchResult::Stop);
        }

        remove_expanded_frontier_entries(&mut warmup);
        let Some(login) = warmup.frontier.first().cloned() else {
            let now = Utc::now();
            warmup.status = DiscoveryWarmupStatus::Completed;
            warmup.current_login = None;
            warmup.updated_at = now;
            warmup.completed_at = Some(now);
            self.save_warmup(user_id, warmup, guard.lease()).await?;
            self.complete_warmup_lease(&mut guard).await?;
            return Ok(DiscoveryWarmupBatchResult::Stop);
        };

        let connection = match self.github_connection(user_id).await {
            Ok(connection) => connection,
            Err(error) => {
                let now = Utc::now();
                warmup.status = DiscoveryWarmupStatus::Failed;
                warmup.current_login = None;
                warmup.updated_at = now;
                warmup.completed_at = Some(now);
                warmup.last_error = Some(truncate_warmup_error(&error));
                let save_result = self.save_warmup(user_id, warmup, guard.lease()).await;
                guard.fail(&error).await;
                save_result?;
                return Err(error);
            }
        };

        warmup.status = DiscoveryWarmupStatus::Running;
        warmup.current_login = Some(login.clone());
        warmup.updated_at = Utc::now();
        warmup.completed_at = None;
        warmup.last_error = None;
        self.save_warmup(user_id, warmup.clone(), guard.lease())
            .await?;

        let neighborhood_result = match self.discovery_repo.user_neighborhood(user_id, &login).await
        {
            Ok(neighborhood)
                if neighborhood.user.neighborhood_cache_status == CacheStatus::Fresh
                    && neighborhood.coverage.is_complete() =>
            {
                Ok((neighborhood, false))
            }
            Ok(_) | Err(AppError::NotFound(_)) => fetch_with_refresh_heartbeat(
                self.import_repo.clone(),
                guard.lease(),
                self.expand_user_with_reserve(user_id, &login, GITHUB_CORE_REST_RESERVE),
            )
            .await
            .map(|neighborhood| (neighborhood, true)),
            Err(error) => Err(error),
        };

        match neighborhood_result {
            Ok((neighborhood, refreshed)) => {
                advance_warmup_frontier(&mut warmup, &login, &neighborhood);
                let remaining_after_refresh = if refreshed {
                    match self
                        .identity_repo
                        .github_rate_limit(connection.account.github_user_id)
                        .await
                    {
                        Ok(Some(status)) => {
                            warmup.remaining_requests = Some(status.remaining);
                            warmup.reset_at = Some(status.reset_at);
                            Some(status.remaining)
                        }
                        Ok(None) => None,
                        Err(error) => {
                            tracing::warn!(app_user_id = %user_id, %error, "could not read warmup rate-limit progress");
                            None
                        }
                    }
                } else {
                    None
                };
                let now = Utc::now();
                warmup.current_login = None;
                warmup.updated_at = now;
                let result = if remaining_after_refresh
                    .is_some_and(|remaining| remaining <= warmup.reserve_requests)
                {
                    warmup.status = DiscoveryWarmupStatus::ReserveProtected;
                    warmup.completed_at = Some(now);
                    DiscoveryWarmupBatchResult::Stop
                } else if warmup.frontier.is_empty() {
                    warmup.status = DiscoveryWarmupStatus::Completed;
                    warmup.completed_at = Some(now);
                    DiscoveryWarmupBatchResult::Stop
                } else {
                    warmup.status = DiscoveryWarmupStatus::Queued;
                    warmup.completed_at = None;
                    DiscoveryWarmupBatchResult::Continue
                };
                self.save_warmup(user_id, warmup, guard.lease()).await?;
                self.complete_warmup_lease(&mut guard).await?;
                Ok(result)
            }
            Err(
                error @ AppError::RateBudgetReserved {
                    remaining,
                    reset_at,
                    ..
                },
            ) => {
                let now = Utc::now();
                warmup.status = DiscoveryWarmupStatus::ReserveProtected;
                warmup.current_login = None;
                warmup.remaining_requests = Some(remaining);
                warmup.reset_at = Some(reset_at);
                warmup.updated_at = now;
                warmup.completed_at = Some(now);
                warmup.last_error = None;
                self.save_warmup(user_id, warmup, guard.lease()).await?;
                self.complete_warmup_lease(&mut guard).await?;
                tracing::info!(app_user_id = %user_id, %error, "discovery warmup preserved the GitHub REST reserve");
                Ok(DiscoveryWarmupBatchResult::Stop)
            }
            Err(error) => {
                let now = Utc::now();
                warmup.status = DiscoveryWarmupStatus::Failed;
                warmup.current_login = None;
                warmup.updated_at = now;
                warmup.completed_at = Some(now);
                warmup.last_error = Some(truncate_warmup_error(&error));
                let save_result = self.save_warmup(user_id, warmup, guard.lease()).await;
                guard.fail(&error).await;
                save_result?;
                Err(error)
            }
        }
    }

    async fn save_warmup(
        &self,
        user_id: &str,
        warmup: DiscoveryWarmupJob,
        lease: &RefreshLease,
    ) -> AppResult<()> {
        if !self
            .sync_state_repo
            .save_discovery_warmup_under_lease(user_id, warmup, lease)
            .await?
        {
            return Err(AppError::External(
                "discovery warmup lease was lost before progress could be saved".to_string(),
            ));
        }
        Ok(())
    }

    async fn complete_warmup_lease(&self, guard: &mut DurableRefreshGuard) -> AppResult<()> {
        if !self
            .import_repo
            .complete_refresh_lease(guard.lease(), None)
            .await?
        {
            return Err(AppError::External(
                "discovery warmup lease was lost before batch completion".to_string(),
            ));
        }
        guard.disarm();
        Ok(())
    }
}

fn warmup_refresh_key(user_id: &str) -> String {
    format!("discovery-warmup:{user_id}")
}

fn remove_expanded_frontier_entries(warmup: &mut DiscoveryWarmupJob) {
    let expanded = warmup
        .expanded_logins
        .iter()
        .map(|login| login.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    warmup
        .frontier
        .retain(|login| !expanded.contains(&login.to_ascii_lowercase()));
}

fn advance_warmup_frontier(
    warmup: &mut DiscoveryWarmupJob,
    requested_login: &str,
    neighborhood: &UserNeighborhood,
) {
    let requested_login = requested_login.to_ascii_lowercase();
    let canonical_login = neighborhood.user.profile.login.to_ascii_lowercase();
    warmup.frontier.retain(|login| {
        let login = login.to_ascii_lowercase();
        login != requested_login && login != canonical_login
    });
    if !warmup
        .expanded_logins
        .iter()
        .any(|login| login.eq_ignore_ascii_case(&canonical_login))
    {
        warmup.expanded_logins.push(canonical_login.clone());
    }

    let mut known = warmup
        .expanded_logins
        .iter()
        .chain(warmup.frontier.iter())
        .map(|login| login.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    for candidate in neighborhood
        .following
        .iter()
        .chain(neighborhood.followers.iter())
    {
        let Ok(login) = normalize_github_login(&candidate.profile.login) else {
            continue;
        };
        if known.contains(&login) {
            continue;
        }
        if known.len() >= DISCOVERY_WARMUP_TOTAL_USER_LIMIT {
            warmup.frontier_truncated = true;
            continue;
        }
        known.insert(login.clone());
        warmup.frontier.push(login);
    }
}

fn truncate_warmup_error(error: &AppError) -> String {
    error.to_string().chars().take(1_000).collect()
}

pub struct DefaultInsightService {
    identity_repo: Arc<dyn IdentityRepository>,
    import_repo: Arc<dyn GitHubImportRepository>,
    insight_repo: Arc<dyn InsightRepository>,
    github: Arc<dyn GitHubClientPort>,
    rate_budgets: GitHubRateBudgetCoordinator,
    cold_refreshes: InsightColdRefreshCoordinator,
}

#[async_trait]
impl InsightService for DefaultInsightService {
    async fn repository_contributors(
        &self,
        user_id: &str,
        full_name: &str,
        limit: usize,
    ) -> AppResult<RepositoryContributorInsights> {
        ensure_insight_limit(limit)?;
        let full_name = full_name.trim();
        let Some((owner, name)) = full_name.split_once('/') else {
            return Err(AppError::Validation(
                "repository name must use the owner/name form".to_string(),
            ));
        };
        ensure(
            !owner.is_empty() && !name.is_empty() && !name.contains('/'),
            "repository name must use the owner/name form",
        )?;
        self.import_repo
            .resolve_bookmark_target(&BookmarkTarget::GitHubRepository {
                full_name: full_name.to_string(),
            })
            .await?;
        let connection = self.github_connection(user_id).await?;
        let now = Utc::now();

        if let Some(mut cached) = self.insight_repo.repository_contributors(full_name).await? {
            if cached.cache_status(now) != crate::graph::CacheStatus::Fresh
                && self
                    .insight_repo
                    .begin_repository_contributor_refresh(full_name)
                    .await?
            {
                cached.cache.refresh_started_at = Some(now);
                cached.cache.last_refresh_error = None;
                spawn_repository_contributor_refresh(
                    self.insight_repo.clone(),
                    self.github.clone(),
                    self.rate_budgets.clone(),
                    connection,
                    full_name.to_string(),
                );
            }
            return Ok(cached.limited(limit));
        }

        let refresh_key = format!(
            "github-repository-contributors:{}",
            full_name.to_ascii_lowercase()
        );
        let gate = self.cold_refreshes.gate(&refresh_key).await;
        let gate_guard = gate.lock().await;
        let result = match self.insight_repo.repository_contributors(full_name).await {
            Ok(Some(cached)) => Ok(cached),
            Ok(None) => {
                refresh_repository_contributors_cold(
                    self.import_repo.clone(),
                    self.insight_repo.clone(),
                    self.github.clone(),
                    self.rate_budgets.clone(),
                    connection,
                    full_name.to_string(),
                )
                .await
            }
            Err(error) => Err(error),
        };
        drop(gate_guard);
        result.map(|insights| insights.limited(limit))
    }

    async fn user_commit_repositories(
        &self,
        user_id: &str,
        login: &str,
        limit: usize,
    ) -> AppResult<UserCommitRepositoryInsights> {
        ensure_insight_limit(limit)?;
        let login = normalize_github_login(login)?;
        self.import_repo
            .resolve_bookmark_target(&BookmarkTarget::GitHubUser {
                login: login.clone(),
            })
            .await?;
        let connection = self.github_connection(user_id).await?;
        let now = Utc::now();

        if let Some(mut cached) = self.insight_repo.user_commit_repositories(&login).await? {
            if cached.cache_status(now) != crate::graph::CacheStatus::Fresh
                && self
                    .insight_repo
                    .begin_user_commit_repository_refresh(&login)
                    .await?
            {
                cached.cache.refresh_started_at = Some(now);
                cached.cache.last_refresh_error = None;
                spawn_user_commit_repository_refresh(
                    self.insight_repo.clone(),
                    self.github.clone(),
                    self.rate_budgets.clone(),
                    connection,
                    login,
                );
            }
            return Ok(cached.limited(limit));
        }

        let refresh_key = format!("github-user-commit-activity:{login}");
        let gate = self.cold_refreshes.gate(&refresh_key).await;
        let gate_guard = gate.lock().await;
        let result = match self.insight_repo.user_commit_repositories(&login).await {
            Ok(Some(cached)) => Ok(cached),
            Ok(None) => {
                refresh_user_commit_repositories_cold(
                    self.import_repo.clone(),
                    self.insight_repo.clone(),
                    self.github.clone(),
                    self.rate_budgets.clone(),
                    connection,
                    login,
                )
                .await
            }
            Err(error) => Err(error),
        };
        drop(gate_guard);
        result.map(|insights| insights.limited(limit))
    }
}

impl DefaultInsightService {
    async fn github_connection(&self, user_id: &str) -> AppResult<GitHubConnection> {
        self.identity_repo
            .get_connection(user_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation("user is not authenticated with GitHub".to_string())
            })
    }
}

fn ensure_insight_limit(limit: usize) -> AppResult<()> {
    ensure(
        (1..=100).contains(&limit),
        "insight result limit must be between 1 and 100",
    )
}

fn normalize_github_login(login: &str) -> AppResult<String> {
    let login = login.trim();
    let bytes = login.as_bytes();
    let valid = (1..=39).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && !bytes.windows(2).any(|pair| pair == b"--");
    ensure(
        valid,
        "github login must be 1-39 ASCII letters, digits, or single hyphens and cannot begin or end with a hyphen",
    )?;
    Ok(login.to_ascii_lowercase())
}

fn ensure_distinct_exploration_trail(trail: &[String]) -> AppResult<()> {
    ensure(
        trail.iter().collect::<HashSet<_>>().len() == trail.len(),
        "exploration trail cannot repeat a person",
    )
}

async fn refresh_repository_contributors(
    insight_repo: Arc<dyn InsightRepository>,
    github: Arc<dyn GitHubClientPort>,
    rate_budgets: GitHubRateBudgetCoordinator,
    connection: GitHubConnection,
    full_name: String,
) -> AppResult<RepositoryContributorInsights> {
    let budget_guard = rate_budgets
        .begin(
            &connection,
            "repository contributor refresh",
            GITHUB_REPOSITORY_CONTRIBUTORS_MAX_REQUESTS,
        )
        .await?;
    let snapshot_result = fetch_with_rate_budget_heartbeat(
        &budget_guard,
        github.fetch_repository_contributors(&connection, &full_name),
    )
    .await;
    budget_guard.finish().await;
    let snapshot = snapshot_result?;
    let insights = RepositoryContributorInsights::from_snapshot(full_name, snapshot, Utc::now());
    insight_repo
        .save_repository_contributors(insights.clone())
        .await?;
    Ok(insights)
}

async fn refresh_repository_contributors_cold(
    import_repo: Arc<dyn GitHubImportRepository>,
    insight_repo: Arc<dyn InsightRepository>,
    github: Arc<dyn GitHubClientPort>,
    rate_budgets: GitHubRateBudgetCoordinator,
    connection: GitHubConnection,
    full_name: String,
) -> AppResult<RepositoryContributorInsights> {
    let entity_key = format!(
        "github-repository-contributors:{}",
        full_name.to_ascii_lowercase()
    );
    match enter_durable_refresh(&import_repo, &entity_key).await? {
        DurableRefreshRole::Completed(_) => insight_repo
            .repository_contributors(&full_name)
            .await?
            .ok_or_else(|| {
                AppError::Storage(
                    "completed contributor refresh did not produce a cache entry".to_string(),
                )
            }),
        DurableRefreshRole::Leader(lease) => {
            let mut guard = DurableRefreshGuard::new(import_repo.clone(), lease);
            let budget_guard = match begin_rate_budget_under_refresh_lease(
                &rate_budgets,
                import_repo.clone(),
                guard.lease(),
                &connection,
                "repository contributor refresh",
                GITHUB_REPOSITORY_CONTRIBUTORS_MAX_REQUESTS,
                GITHUB_CORE_REST_RESERVE,
            )
            .await
            {
                Ok(budget_guard) => budget_guard,
                Err(error) => {
                    guard.fail(&error).await;
                    return Err(error);
                }
            };
            let snapshot_result = fetch_with_refresh_heartbeat(
                import_repo.clone(),
                guard.lease(),
                fetch_with_rate_budget_heartbeat(
                    &budget_guard,
                    github.fetch_repository_contributors(&connection, &full_name),
                ),
            )
            .await;
            budget_guard.finish().await;
            let snapshot = match snapshot_result {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    guard.fail(&error).await;
                    return Err(error);
                }
            };
            if !import_repo
                .renew_refresh_lease(guard.lease(), REFRESH_LEASE_SECONDS)
                .await?
            {
                let error = AppError::External(
                    "contributor refresh lease was lost before cache write".to_string(),
                );
                guard.fail(&error).await;
                return Err(error);
            }
            let insights =
                RepositoryContributorInsights::from_snapshot(full_name, snapshot, Utc::now());
            if let Err(error) = insight_repo
                .save_repository_contributors(insights.clone())
                .await
            {
                guard.fail(&error).await;
                return Err(error);
            }
            if !import_repo
                .complete_refresh_lease(guard.lease(), None)
                .await?
            {
                let error = AppError::External(
                    "contributor refresh lease was lost before completion".to_string(),
                );
                guard.fail(&error).await;
                return Err(error);
            }
            guard.disarm();
            Ok(insights)
        }
    }
}

fn spawn_repository_contributor_refresh(
    insight_repo: Arc<dyn InsightRepository>,
    github: Arc<dyn GitHubClientPort>,
    rate_budgets: GitHubRateBudgetCoordinator,
    connection: GitHubConnection,
    full_name: String,
) {
    tokio::spawn(async move {
        if let Err(error) = refresh_repository_contributors(
            insight_repo.clone(),
            github,
            rate_budgets,
            connection,
            full_name.clone(),
        )
        .await
        {
            let _ = insight_repo
                .fail_repository_contributor_refresh(&full_name, &error.to_string())
                .await;
            tracing::warn!(repository = %full_name, %error, "repository contributor refresh failed");
        }
    });
}

async fn refresh_user_commit_repositories(
    insight_repo: Arc<dyn InsightRepository>,
    github: Arc<dyn GitHubClientPort>,
    rate_budgets: GitHubRateBudgetCoordinator,
    connection: GitHubConnection,
    login: String,
) -> AppResult<UserCommitRepositoryInsights> {
    let budget_guard = rate_budgets
        .begin(
            &connection,
            "user commit-activity refresh",
            GITHUB_USER_EVENTS_MAX_REQUESTS,
        )
        .await?;
    let snapshot_result = fetch_with_rate_budget_heartbeat(
        &budget_guard,
        github.fetch_user_commit_repositories(&connection, &login),
    )
    .await;
    budget_guard.finish().await;
    let snapshot = snapshot_result?;
    let insights = UserCommitRepositoryInsights::from_snapshot(login, snapshot, Utc::now());
    insight_repo
        .save_user_commit_repositories(insights.clone())
        .await?;
    Ok(insights)
}

async fn refresh_user_commit_repositories_cold(
    import_repo: Arc<dyn GitHubImportRepository>,
    insight_repo: Arc<dyn InsightRepository>,
    github: Arc<dyn GitHubClientPort>,
    rate_budgets: GitHubRateBudgetCoordinator,
    connection: GitHubConnection,
    login: String,
) -> AppResult<UserCommitRepositoryInsights> {
    let entity_key = format!("github-user-commit-activity:{login}");
    match enter_durable_refresh(&import_repo, &entity_key).await? {
        DurableRefreshRole::Completed(_) => insight_repo
            .user_commit_repositories(&login)
            .await?
            .ok_or_else(|| {
                AppError::Storage(
                    "completed commit-activity refresh did not produce a cache entry".to_string(),
                )
            }),
        DurableRefreshRole::Leader(lease) => {
            let mut guard = DurableRefreshGuard::new(import_repo.clone(), lease);
            let budget_guard = match begin_rate_budget_under_refresh_lease(
                &rate_budgets,
                import_repo.clone(),
                guard.lease(),
                &connection,
                "user commit-activity refresh",
                GITHUB_USER_EVENTS_MAX_REQUESTS,
                GITHUB_CORE_REST_RESERVE,
            )
            .await
            {
                Ok(budget_guard) => budget_guard,
                Err(error) => {
                    guard.fail(&error).await;
                    return Err(error);
                }
            };
            let snapshot_result = fetch_with_refresh_heartbeat(
                import_repo.clone(),
                guard.lease(),
                fetch_with_rate_budget_heartbeat(
                    &budget_guard,
                    github.fetch_user_commit_repositories(&connection, &login),
                ),
            )
            .await;
            budget_guard.finish().await;
            let snapshot = match snapshot_result {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    guard.fail(&error).await;
                    return Err(error);
                }
            };
            if !import_repo
                .renew_refresh_lease(guard.lease(), REFRESH_LEASE_SECONDS)
                .await?
            {
                let error = AppError::External(
                    "commit-activity refresh lease was lost before cache write".to_string(),
                );
                guard.fail(&error).await;
                return Err(error);
            }
            let insights = UserCommitRepositoryInsights::from_snapshot(login, snapshot, Utc::now());
            if let Err(error) = insight_repo
                .save_user_commit_repositories(insights.clone())
                .await
            {
                guard.fail(&error).await;
                return Err(error);
            }
            if !import_repo
                .complete_refresh_lease(guard.lease(), None)
                .await?
            {
                let error = AppError::External(
                    "commit-activity refresh lease was lost before completion".to_string(),
                );
                guard.fail(&error).await;
                return Err(error);
            }
            guard.disarm();
            Ok(insights)
        }
    }
}

fn spawn_user_commit_repository_refresh(
    insight_repo: Arc<dyn InsightRepository>,
    github: Arc<dyn GitHubClientPort>,
    rate_budgets: GitHubRateBudgetCoordinator,
    connection: GitHubConnection,
    login: String,
) {
    tokio::spawn(async move {
        if let Err(error) = refresh_user_commit_repositories(
            insight_repo.clone(),
            github,
            rate_budgets,
            connection,
            login.clone(),
        )
        .await
        {
            let _ = insight_repo
                .fail_user_commit_repository_refresh(&login, &error.to_string())
                .await;
            tracing::warn!(github_login = %login, %error, "user commit insight refresh failed");
        }
    });
}

#[derive(Clone, Default)]
struct InsightColdRefreshCoordinator {
    gates: Arc<AsyncMutex<HashMap<String, Weak<AsyncMutex<()>>>>>,
}

impl InsightColdRefreshCoordinator {
    async fn gate(&self, key: &str) -> Arc<AsyncMutex<()>> {
        let mut gates = self.gates.lock().await;
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(key).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(AsyncMutex::new(()));
        gates.insert(key.to_string(), Arc::downgrade(&gate));
        gate
    }
}

#[derive(Clone, Default)]
struct RefreshCoordinator {
    flights: Arc<AsyncMutex<HashMap<String, Arc<RefreshFlight>>>>,
}

struct RefreshFlight {
    result: watch::Sender<Option<Result<SyncSummary, String>>>,
}

struct RefreshLeaderGuard {
    coordinator: RefreshCoordinator,
    entity_key: String,
    flight: Arc<RefreshFlight>,
    armed: bool,
}

impl RefreshLeaderGuard {
    fn new(
        coordinator: RefreshCoordinator,
        entity_key: String,
        flight: Arc<RefreshFlight>,
    ) -> Self {
        Self {
            coordinator,
            entity_key,
            flight,
            armed: true,
        }
    }

    async fn finish(mut self, result: &AppResult<SyncSummary>) {
        self.coordinator
            .finish(&self.entity_key, &self.flight, result)
            .await;
        self.armed = false;
    }
}

impl Drop for RefreshLeaderGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if self.flight.result.borrow().is_none() {
            self.flight.result.send_replace(Some(Err(
                "refresh leader was cancelled before completion".to_string(),
            )));
        }
        let coordinator = self.coordinator.clone();
        let entity_key = self.entity_key.clone();
        let flight = self.flight.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                coordinator.remove_if_current(&entity_key, &flight).await;
            });
        }
    }
}

impl RefreshCoordinator {
    async fn join(&self, entity_key: &str) -> (Arc<RefreshFlight>, bool) {
        let mut flights = self.flights.lock().await;
        if let Some(flight) = flights.get(entity_key) {
            return (flight.clone(), false);
        }
        let (result, _) = watch::channel(None);
        let flight = Arc::new(RefreshFlight { result });
        flights.insert(entity_key.to_string(), flight.clone());
        (flight, true)
    }

    async fn wait(flight: &RefreshFlight) -> AppResult<SyncSummary> {
        let mut result = flight.result.subscribe();
        loop {
            if let Some(completed) = result.borrow().clone() {
                return completed.map_err(|error| {
                    AppError::External(format!("deduplicated refresh failed: {error}"))
                });
            }
            result.changed().await.map_err(|_| {
                AppError::External("deduplicated refresh ended without a result".to_string())
            })?;
        }
    }

    async fn finish(
        &self,
        entity_key: &str,
        flight: &Arc<RefreshFlight>,
        result: &AppResult<SyncSummary>,
    ) {
        let shared_result = result.as_ref().cloned().map_err(ToString::to_string);
        flight.result.send_replace(Some(shared_result));
        self.remove_if_current(entity_key, flight).await;
    }

    async fn remove_if_current(&self, entity_key: &str, flight: &Arc<RefreshFlight>) {
        let mut flights = self.flights.lock().await;
        if flights
            .get(entity_key)
            .is_some_and(|current| Arc::ptr_eq(current, flight))
        {
            flights.remove(entity_key);
        }
    }
}

fn refresh_key(login: &str) -> String {
    format!("github-user:{}", login.trim().to_ascii_lowercase())
}

const REFRESH_LEASE_SECONDS: i64 = 300;
const REFRESH_HEARTBEAT_SECONDS: u64 = 60;
const REFRESH_WAIT_SECONDS: u64 = 360;

enum DurableRefreshRole {
    Leader(RefreshLease),
    Completed(Option<String>),
}

async fn enter_durable_refresh(
    import_repo: &Arc<dyn GitHubImportRepository>,
    entity_key: &str,
) -> AppResult<DurableRefreshRole> {
    let deadline = Instant::now() + StdDuration::from_secs(REFRESH_WAIT_SECONDS);
    loop {
        let token = Uuid::new_v4().to_string();
        match import_repo
            .try_acquire_refresh_lease(entity_key, &token, REFRESH_LEASE_SECONDS)
            .await?
        {
            RefreshLeaseAttempt::Acquired(lease) => {
                return Ok(DurableRefreshRole::Leader(lease));
            }
            RefreshLeaseAttempt::Busy(mut state) => loop {
                match state.status {
                    RefreshLeaseStatus::Succeeded => {
                        return Ok(DurableRefreshRole::Completed(state.outcome_json));
                    }
                    RefreshLeaseStatus::Failed => {
                        return Err(AppError::External(format!(
                            "deduplicated refresh failed: {}",
                            state
                                .last_error
                                .unwrap_or_else(|| "refresh failed without an error".to_string())
                        )));
                    }
                    RefreshLeaseStatus::Running if state.expired => break,
                    RefreshLeaseStatus::Running => {}
                }
                if Instant::now() >= deadline {
                    return Err(AppError::External(
                        "timed out waiting for the shared refresh lease".to_string(),
                    ));
                }
                time::sleep(StdDuration::from_millis(500)).await;
                match import_repo.refresh_lease_state(entity_key).await? {
                    Some(current) => state = current,
                    None => break,
                }
            },
        }
    }
}

async fn begin_rate_budget_under_refresh_lease(
    rate_budgets: &GitHubRateBudgetCoordinator,
    import_repo: Arc<dyn GitHubImportRepository>,
    refresh_lease: &RefreshLease,
    connection: &GitHubConnection,
    operation: &str,
    requested_cost: usize,
    reserve: usize,
) -> AppResult<GitHubRateBudgetGuard> {
    fetch_with_refresh_heartbeat(
        import_repo,
        refresh_lease,
        rate_budgets.begin_with_reserve(connection, operation, requested_cost, reserve),
    )
    .await
}

async fn fetch_with_refresh_heartbeat<T, F>(
    import_repo: Arc<dyn GitHubImportRepository>,
    lease: &RefreshLease,
    fetch: F,
) -> AppResult<T>
where
    F: Future<Output = AppResult<T>> + Send,
{
    tokio::pin!(fetch);
    let heartbeat = time::sleep(StdDuration::from_secs(REFRESH_HEARTBEAT_SECONDS));
    tokio::pin!(heartbeat);
    loop {
        tokio::select! {
            result = &mut fetch => return result,
            () = &mut heartbeat => {
                if !import_repo
                    .renew_refresh_lease(lease, REFRESH_LEASE_SECONDS)
                    .await?
                {
                    return Err(AppError::External(
                        "refresh lease was lost while fetching GitHub data".to_string(),
                    ));
                }
                heartbeat.as_mut().reset(
                    Instant::now() + StdDuration::from_secs(REFRESH_HEARTBEAT_SECONDS),
                );
            }
        }
    }
}

async fn fetch_with_rate_budget_heartbeat<T, F>(
    guard: &GitHubRateBudgetGuard,
    fetch: F,
) -> AppResult<T>
where
    F: Future<Output = AppResult<T>> + Send,
{
    tokio::pin!(fetch);
    let heartbeat = time::sleep(StdDuration::from_secs(RATE_BUDGET_HEARTBEAT_SECONDS));
    tokio::pin!(heartbeat);
    loop {
        tokio::select! {
            result = &mut fetch => return result,
            () = &mut heartbeat => {
                if !guard.renew().await? {
                    return Err(AppError::External(
                        "GitHub rate-budget lease was lost while fetching data".to_string(),
                    ));
                }
                heartbeat.as_mut().reset(
                    Instant::now() + StdDuration::from_secs(RATE_BUDGET_HEARTBEAT_SECONDS),
                );
            }
        }
    }
}

struct DurableRefreshGuard {
    import_repo: Arc<dyn GitHubImportRepository>,
    lease: Option<RefreshLease>,
}

impl DurableRefreshGuard {
    fn new(import_repo: Arc<dyn GitHubImportRepository>, lease: RefreshLease) -> Self {
        Self {
            import_repo,
            lease: Some(lease),
        }
    }

    fn lease(&self) -> &RefreshLease {
        self.lease.as_ref().expect("refresh guard is armed")
    }

    fn disarm(&mut self) {
        self.lease = None;
    }

    async fn fail(mut self, error: &AppError) {
        if let Some(lease) = self.lease.take() {
            let _ = self
                .import_repo
                .fail_refresh_lease(&lease, &error.to_string())
                .await;
        }
    }
}

impl Drop for DurableRefreshGuard {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        let import_repo = self.import_repo.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = import_repo
                    .fail_refresh_lease(&lease, "refresh leader was cancelled before completion")
                    .await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DISCOVERY_WARMUP_TOTAL_USER_LIMIT, advance_warmup_frontier,
        ensure_distinct_exploration_trail, normalize_github_login,
    };
    use crate::{
        discovery::{DiscoveryUser, UserNeighborhood},
        graph::{
            CacheStatus, DiscoveryWarmupJob, DiscoveryWarmupStatus, GitHubUserNode,
            GraphImportCoverage,
        },
        shared::AppError,
    };

    #[test]
    fn github_login_validation_rejects_path_and_query_delimiters() {
        for invalid in [
            "owner/name",
            "alice?tab=repositories",
            "alice#fragment",
            ".",
            "..",
            "-alice",
            "alice-",
            "alice--bob",
            "",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(
                matches!(
                    normalize_github_login(invalid),
                    Err(AppError::Validation(_))
                ),
                "`{invalid}` must not be accepted as a GitHub login"
            );
        }
    }

    #[test]
    fn github_login_validation_normalizes_valid_ascii_logins() {
        assert_eq!(
            normalize_github_login(" Octo-Cat1 ").expect("valid GitHub login"),
            "octo-cat1"
        );
    }

    #[test]
    fn exploration_trail_rejects_any_repeated_login() {
        let trail = vec!["alice".to_string(), "bob".to_string(), "alice".to_string()];
        assert!(matches!(
            ensure_distinct_exploration_trail(&trail),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn discovery_warmup_total_bound_truncates_and_exhausts_the_frontier() {
        let now = chrono::Utc::now();
        let mut warmup = DiscoveryWarmupJob {
            id: "bounded-warmup".to_string(),
            seed_login: "seed-0".to_string(),
            status: DiscoveryWarmupStatus::Running,
            current_login: Some("current".to_string()),
            expanded_logins: (0..DISCOVERY_WARMUP_TOTAL_USER_LIMIT - 1)
                .map(|index| format!("seed-{index}"))
                .collect(),
            frontier: vec!["current".to_string()],
            frontier_truncated: false,
            remaining_requests: None,
            reserve_requests: 1_000,
            reset_at: None,
            started_at: now,
            updated_at: now,
            completed_at: None,
            last_error: None,
        };
        let neighborhood = UserNeighborhood {
            user: DiscoveryUser {
                profile: GitHubUserNode {
                    github_id: 1,
                    login: "current".to_string(),
                    url: "https://github.com/current".to_string(),
                    ..Default::default()
                },
                neighborhood_cache_status: CacheStatus::Fresh,
                neighborhood_last_fetched_at: Some(now),
            },
            followers: Vec::new(),
            following: vec![DiscoveryUser {
                profile: GitHubUserNode {
                    github_id: 2,
                    login: "overflow".to_string(),
                    url: "https://github.com/overflow".to_string(),
                    ..Default::default()
                },
                neighborhood_cache_status: CacheStatus::Stale,
                neighborhood_last_fetched_at: None,
            }],
            starred_repositories: Vec::new(),
            owned_repositories: Vec::new(),
            coverage: GraphImportCoverage::default(),
        };

        advance_warmup_frontier(&mut warmup, "current", &neighborhood);

        assert_eq!(warmup.discovered_users(), DISCOVERY_WARMUP_TOTAL_USER_LIMIT);
        assert!(warmup.frontier.is_empty());
        assert!(warmup.frontier_truncated);
    }
}
