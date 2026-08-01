use std::collections::HashMap;

use async_graphql::{
    Context, EmptySubscription, Enum, Error, ErrorExtensions, Object, Result as GraphQlResult,
    Schema, SimpleObject,
};

use crate::{
    bookmarks::{Bookmark, BookmarkTarget},
    bootstrap::AppState,
    discovery::{DiscoveryRepositoryRecord, DiscoveryUser, RepositoryCandidate, UserNeighborhood},
    graph::{
        CacheStatus, DiscoveryWarmupJob, DiscoveryWarmupStatus, GitHubRateLimitStatus,
        GitHubRepositoryNode, GitHubUserNode, GraphImportCoverage,
    },
    insights::{
        REPOSITORY_CONTRIBUTOR_SOURCE, RepositoryContributor, RepositoryContributorInsights,
        USER_COMMIT_ACTIVITY_SOURCE, USER_COMMIT_ACTIVITY_WINDOW_DAYS, UserCommitRepository,
        UserCommitRepositoryInsights,
    },
    shared::{AppError, Shared},
};

pub type GitExploreSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

#[derive(Clone)]
pub struct GraphQlSession {
    pub user_id: Option<String>,
}

pub fn build_schema(state: Shared<AppState>) -> GitExploreSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .limit_depth(12)
        .limit_complexity(100)
        .data(state)
        .finish()
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn api_version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    #[graphql(complexity = 20)]
    async fn neighborhood(
        &self,
        context: &Context<'_>,
        login: String,
        limit: i32,
    ) -> GraphQlResult<UserNeighborhoodObject> {
        let user_id = request_user_id(context)?;
        let limit = repository_limit(limit)?;
        let state = context.data::<Shared<AppState>>()?;
        let neighborhood = state
            .services
            .discovery
            .user_neighborhood(user_id, &login)
            .await
            .map_err(graphql_error)?;
        let repositories = state
            .services
            .discovery
            .discover_repositories(user_id, &login, limit)
            .await
            .map_err(graphql_error)?;
        Ok(UserNeighborhoodObject::from_domain(
            neighborhood,
            repositories,
            limit,
        ))
    }

    #[graphql(complexity = 2)]
    async fn rate_limit(&self, context: &Context<'_>) -> GraphQlResult<GitHubRateLimitObject> {
        let user_id = request_user_id(context)?;
        let state = context.data::<Shared<AppState>>()?;
        state
            .services
            .sync
            .rate_limit(user_id)
            .await
            .map(GitHubRateLimitObject::from)
            .map_err(graphql_error)
    }

    #[graphql(complexity = 2)]
    async fn discovery_warmup(
        &self,
        context: &Context<'_>,
    ) -> GraphQlResult<Option<DiscoveryWarmupObject>> {
        let user_id = request_user_id(context)?;
        let state = context.data::<Shared<AppState>>()?;
        state
            .services
            .discovery
            .warmup_status(user_id)
            .await
            .map(|warmup| warmup.map(DiscoveryWarmupObject::from))
            .map_err(graphql_error)
    }

    #[graphql(complexity = 12)]
    async fn repository_insights(
        &self,
        context: &Context<'_>,
        full_name: String,
        limit: i32,
    ) -> GraphQlResult<RepositoryContributorInsightsObject> {
        let user_id = request_user_id(context)?;
        let limit = insight_limit(limit)?;
        let state = context.data::<Shared<AppState>>()?;
        state
            .services
            .insights
            .repository_contributors(user_id, &full_name, limit)
            .await
            .map(RepositoryContributorInsightsObject::from)
            .map_err(graphql_error)
    }

    #[graphql(complexity = 12)]
    async fn user_insights(
        &self,
        context: &Context<'_>,
        login: String,
        limit: i32,
    ) -> GraphQlResult<UserCommitRepositoryInsightsObject> {
        let user_id = request_user_id(context)?;
        let limit = insight_limit(limit)?;
        let state = context.data::<Shared<AppState>>()?;
        state
            .services
            .insights
            .user_commit_repositories(user_id, &login, limit)
            .await
            .map(UserCommitRepositoryInsightsObject::from)
            .map_err(graphql_error)
    }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    #[graphql(complexity = 2)]
    async fn start_discovery_warmup(
        &self,
        context: &Context<'_>,
    ) -> GraphQlResult<DiscoveryWarmupObject> {
        let user_id = request_user_id(context)?;
        let state = context.data::<Shared<AppState>>()?;
        state
            .services
            .discovery
            .start_warmup(user_id)
            .await
            .map(DiscoveryWarmupObject::from)
            .map_err(graphql_error)
    }

    #[graphql(complexity = 60)]
    async fn expand_user(
        &self,
        context: &Context<'_>,
        login: String,
        limit: i32,
    ) -> GraphQlResult<UserNeighborhoodObject> {
        let user_id = request_user_id(context)?;
        let limit = repository_limit(limit)?;
        let state = context.data::<Shared<AppState>>()?;
        let neighborhood = state
            .services
            .discovery
            .expand_user(user_id, &login)
            .await
            .map_err(graphql_error)?;
        let repositories = state
            .services
            .discovery
            .discover_repositories(user_id, &login, limit)
            .await
            .map_err(graphql_error)?;
        Ok(UserNeighborhoodObject::from_domain(
            neighborhood,
            repositories,
            limit,
        ))
    }

    #[graphql(complexity = 10)]
    async fn save_repository(
        &self,
        context: &Context<'_>,
        full_name: String,
        categories: Vec<String>,
        note: Option<String>,
    ) -> GraphQlResult<SavedRepositoryObject> {
        let user_id = request_user_id(context)?;
        let state = context.data::<Shared<AppState>>()?;
        let bookmark = state
            .services
            .bookmarks
            .add_bookmark(
                user_id,
                BookmarkTarget::GitHubRepository {
                    full_name: full_name.trim().to_string(),
                },
                categories,
                note,
            )
            .await
            .map_err(graphql_error)?;
        SavedRepositoryObject::try_from(bookmark).map_err(graphql_error)
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum CacheStatusObject {
    Fresh,
    Stale,
    Refreshing,
    RefreshFailed,
}

impl From<CacheStatus> for CacheStatusObject {
    fn from(value: CacheStatus) -> Self {
        match value {
            CacheStatus::Fresh => Self::Fresh,
            CacheStatus::Stale => Self::Stale,
            CacheStatus::Refreshing => Self::Refreshing,
            CacheStatus::RefreshFailed => Self::RefreshFailed,
        }
    }
}

#[derive(SimpleObject)]
pub struct GitHubRateLimitObject {
    pub limit: i32,
    pub used: i32,
    pub remaining: i32,
    pub reset_at: String,
    pub checked_at: String,
}

impl From<GitHubRateLimitStatus> for GitHubRateLimitObject {
    fn from(value: GitHubRateLimitStatus) -> Self {
        Self {
            limit: saturating_i32(value.limit as u64),
            used: saturating_i32(value.used as u64),
            remaining: saturating_i32(value.remaining as u64),
            reset_at: value.reset_at.to_rfc3339(),
            checked_at: value.checked_at.to_rfc3339(),
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum DiscoveryWarmupStatusObject {
    Queued,
    Running,
    Completed,
    ReserveProtected,
    Failed,
}

impl From<DiscoveryWarmupStatus> for DiscoveryWarmupStatusObject {
    fn from(value: DiscoveryWarmupStatus) -> Self {
        match value {
            DiscoveryWarmupStatus::Queued => Self::Queued,
            DiscoveryWarmupStatus::Running => Self::Running,
            DiscoveryWarmupStatus::Completed => Self::Completed,
            DiscoveryWarmupStatus::ReserveProtected => Self::ReserveProtected,
            DiscoveryWarmupStatus::Failed => Self::Failed,
        }
    }
}

#[derive(SimpleObject)]
pub struct DiscoveryWarmupObject {
    pub id: String,
    pub seed_login: String,
    pub status: DiscoveryWarmupStatusObject,
    pub current_login: Option<String>,
    pub expanded_users: i32,
    pub discovered_users: i32,
    pub pending_users: i32,
    pub frontier_truncated: bool,
    pub remaining_requests: Option<i32>,
    pub reserve_requests: i32,
    pub reset_at: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub last_error: Option<String>,
}

impl From<DiscoveryWarmupJob> for DiscoveryWarmupObject {
    fn from(value: DiscoveryWarmupJob) -> Self {
        let expanded_users = saturating_i32(value.expanded_users() as u64);
        let discovered_users = saturating_i32(value.discovered_users() as u64);
        let pending_users = saturating_i32(value.pending_users() as u64);
        Self {
            id: value.id,
            seed_login: value.seed_login,
            status: value.status.into(),
            current_login: value.current_login,
            expanded_users,
            discovered_users,
            pending_users,
            frontier_truncated: value.frontier_truncated,
            remaining_requests: value
                .remaining_requests
                .map(|remaining| saturating_i32(remaining as u64)),
            reserve_requests: saturating_i32(value.reserve_requests as u64),
            reset_at: value.reset_at.map(|timestamp| timestamp.to_rfc3339()),
            started_at: value.started_at.to_rfc3339(),
            updated_at: value.updated_at.to_rfc3339(),
            completed_at: value.completed_at.map(|timestamp| timestamp.to_rfc3339()),
            last_error: value.last_error,
        }
    }
}

#[derive(SimpleObject)]
pub struct RepositoryContributorObject {
    pub github_id: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub url: String,
    pub contributions: i32,
}

impl From<RepositoryContributor> for RepositoryContributorObject {
    fn from(value: RepositoryContributor) -> Self {
        Self {
            github_id: value.github_id.to_string(),
            login: value.login,
            avatar_url: value.avatar_url,
            url: value.url,
            contributions: saturating_i32(value.contributions),
        }
    }
}

#[derive(SimpleObject)]
pub struct RepositoryContributorInsightsObject {
    pub full_name: String,
    pub contributors: Vec<RepositoryContributorObject>,
    pub source_complete: bool,
    pub source_description: String,
    pub cache_status: CacheStatusObject,
    pub last_fetched_at: Option<String>,
}

impl From<RepositoryContributorInsights> for RepositoryContributorInsightsObject {
    fn from(value: RepositoryContributorInsights) -> Self {
        let cache_status = value.cache_status(chrono::Utc::now()).into();
        Self {
            full_name: value.full_name,
            contributors: value.contributors.into_iter().map(Into::into).collect(),
            source_complete: value.source_complete,
            source_description: REPOSITORY_CONTRIBUTOR_SOURCE.to_string(),
            cache_status,
            last_fetched_at: value
                .cache
                .last_fetched_at
                .map(|timestamp| timestamp.to_rfc3339()),
        }
    }
}

#[derive(SimpleObject)]
pub struct UserCommitRepositoryObject {
    pub github_id: String,
    pub full_name: String,
    pub url: String,
    pub push_count: i32,
    pub commit_count: i32,
    pub last_pushed_at: String,
}

impl From<UserCommitRepository> for UserCommitRepositoryObject {
    fn from(value: UserCommitRepository) -> Self {
        Self {
            github_id: value.github_id.to_string(),
            full_name: value.full_name,
            url: value.url,
            push_count: saturating_i32(value.push_count),
            commit_count: saturating_i32(value.commit_count),
            last_pushed_at: value.last_pushed_at.to_rfc3339(),
        }
    }
}

#[derive(SimpleObject)]
pub struct UserCommitRepositoryInsightsObject {
    pub login: String,
    pub repositories: Vec<UserCommitRepositoryObject>,
    pub window_days: i32,
    pub source_event_count: i32,
    pub source_truncated: bool,
    pub source_description: String,
    pub cache_status: CacheStatusObject,
    pub last_fetched_at: Option<String>,
}

impl From<UserCommitRepositoryInsights> for UserCommitRepositoryInsightsObject {
    fn from(value: UserCommitRepositoryInsights) -> Self {
        let cache_status = value.cache_status(chrono::Utc::now()).into();
        Self {
            login: value.login,
            repositories: value.repositories.into_iter().map(Into::into).collect(),
            window_days: USER_COMMIT_ACTIVITY_WINDOW_DAYS,
            source_event_count: saturating_i32(value.source_event_count as u64),
            source_truncated: value.source_truncated,
            source_description: USER_COMMIT_ACTIVITY_SOURCE.to_string(),
            cache_status,
            last_fetched_at: value
                .cache
                .last_fetched_at
                .map(|timestamp| timestamp.to_rfc3339()),
        }
    }
}

#[derive(SimpleObject)]
pub struct GraphUserObject {
    pub github_id: String,
    pub login: String,
    pub name: Option<String>,
    pub url: String,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub followers_count: Option<i32>,
    pub following_count: Option<i32>,
}

impl From<GitHubUserNode> for GraphUserObject {
    fn from(value: GitHubUserNode) -> Self {
        Self {
            github_id: value.github_id.to_string(),
            login: value.login,
            name: value.name,
            url: value.url,
            avatar_url: value.avatar_url,
            bio: value.bio,
            followers_count: value.followers_count.map(saturating_i32),
            following_count: value.following_count.map(saturating_i32),
        }
    }
}

impl From<DiscoveryUser> for GraphUserObject {
    fn from(value: DiscoveryUser) -> Self {
        value.profile.into()
    }
}

#[derive(SimpleObject)]
pub struct GraphRepositoryObject {
    pub github_id: String,
    pub owner_login: String,
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub html_url: String,
    pub stargazer_count: i32,
    pub fork_count: i32,
    pub primary_language: Option<String>,
    pub topics: Vec<String>,
    pub updated_at: Option<String>,
    pub archived: bool,
    pub fork: bool,
}

impl From<GitHubRepositoryNode> for GraphRepositoryObject {
    fn from(value: GitHubRepositoryNode) -> Self {
        Self {
            github_id: value.github_id.to_string(),
            owner_login: value.owner_login,
            name: value.name,
            full_name: value.full_name,
            description: value.description,
            html_url: value.html_url,
            stargazer_count: saturating_i32(value.stargazer_count),
            fork_count: saturating_i32(value.fork_count),
            primary_language: value.language,
            topics: value.topics,
            updated_at: value
                .pushed_at
                .or(value.updated_at)
                .map(|timestamp| timestamp.to_rfc3339()),
            archived: value.archived,
            fork: value.is_fork,
        }
    }
}

#[derive(SimpleObject)]
pub struct RepositoryCandidateObject {
    pub repository: GraphRepositoryObject,
    pub network_stars: i32,
    pub via_logins: Vec<String>,
    pub discovery_score: f64,
    pub reasons: Vec<String>,
    pub saved: bool,
}

impl From<RepositoryCandidate> for RepositoryCandidateObject {
    fn from(value: RepositoryCandidate) -> Self {
        Self {
            network_stars: saturating_i32(value.graph_signals.starred_by_recommenders as u64),
            via_logins: value.via_logins,
            discovery_score: value.score,
            reasons: value
                .reasons
                .into_iter()
                .filter(|reason| reason.score > 0.0)
                .map(|reason| reason.description)
                .collect(),
            saved: value.repository.saved,
            repository: value.repository.repository.into(),
        }
    }
}

#[derive(SimpleObject)]
pub struct GraphCoverageObject {
    pub followers_complete: bool,
    pub following_complete: bool,
    pub starred_repositories_complete: bool,
    pub repositories_complete: bool,
}

impl From<GraphImportCoverage> for GraphCoverageObject {
    fn from(value: GraphImportCoverage) -> Self {
        Self {
            followers_complete: value.followers_complete,
            following_complete: value.following_complete,
            starred_repositories_complete: value.starred_repositories_complete,
            repositories_complete: value.repositories_complete,
        }
    }
}

#[derive(SimpleObject)]
pub struct UserNeighborhoodObject {
    pub user: GraphUserObject,
    pub followers: Vec<GraphUserObject>,
    pub following: Vec<GraphUserObject>,
    pub repositories: Vec<RepositoryCandidateObject>,
    pub cache_status: CacheStatusObject,
    pub last_fetched_at: Option<String>,
    pub coverage: GraphCoverageObject,
}

impl UserNeighborhoodObject {
    fn from_domain(
        neighborhood: UserNeighborhood,
        repositories: Vec<RepositoryCandidate>,
        limit: usize,
    ) -> Self {
        let UserNeighborhood {
            user,
            followers,
            following,
            starred_repositories,
            owned_repositories,
            coverage,
        } = neighborhood;
        let seed_login = user.profile.login.clone();
        let cache_status = user.neighborhood_cache_status.into();
        let last_fetched_at = user
            .neighborhood_last_fetched_at
            .map(|timestamp| timestamp.to_rfc3339());
        let mut repository_map = repositories
            .into_iter()
            .map(RepositoryCandidateObject::from)
            .map(|candidate| (candidate.repository.full_name.clone(), candidate))
            .collect::<HashMap<_, _>>();
        for record in owned_repositories {
            let candidate = direct_repository_candidate(record, &seed_login, false);
            repository_map
                .entry(candidate.repository.full_name.clone())
                .or_insert(candidate);
        }
        for record in starred_repositories {
            let candidate = direct_repository_candidate(record, &seed_login, true);
            repository_map
                .entry(candidate.repository.full_name.clone())
                .and_modify(|existing| {
                    existing.discovery_score =
                        existing.discovery_score.max(candidate.discovery_score);
                    existing.network_stars = existing.network_stars.max(1);
                    for reason in &candidate.reasons {
                        if !existing.reasons.contains(reason) {
                            existing.reasons.push(reason.clone());
                        }
                    }
                })
                .or_insert(candidate);
        }
        let mut repositories = repository_map.into_values().collect::<Vec<_>>();
        repositories.sort_by(|left, right| {
            right
                .discovery_score
                .total_cmp(&left.discovery_score)
                .then_with(|| left.repository.full_name.cmp(&right.repository.full_name))
        });
        repositories.truncate(limit);
        Self {
            user: user.into(),
            followers: followers.into_iter().map(Into::into).collect(),
            following: following.into_iter().map(Into::into).collect(),
            repositories,
            cache_status,
            last_fetched_at,
            coverage: coverage.into(),
        }
    }
}

fn direct_repository_candidate(
    record: DiscoveryRepositoryRecord,
    seed_login: &str,
    starred: bool,
) -> RepositoryCandidateObject {
    let repository = record.repository;
    let mut discovery_score = if starred { 18.0 } else { 12.0 };
    let mut reasons = vec![if starred {
        format!("@{seed_login} starred this repository")
    } else {
        format!("@{seed_login} built this repository")
    }];
    if repository.stargazer_count <= 5_000 {
        discovery_score += 4.0;
        reasons.push("A smaller global audience makes this easier to miss".to_string());
    }
    if let Some(activity) = repository.pushed_at.or(repository.updated_at)
        && chrono::Utc::now()
            .signed_duration_since(activity)
            .num_days()
            <= 180
    {
        discovery_score += 2.0;
        reasons.push("Active within the last six months".to_string());
    }
    if repository.archived {
        discovery_score -= 8.0;
        reasons.push("Archived repositories rank lower".to_string());
    }
    RepositoryCandidateObject {
        repository: repository.into(),
        network_stars: if starred { 1 } else { 0 },
        via_logins: vec![seed_login.to_string()],
        discovery_score,
        reasons,
        saved: record.saved,
    }
}

#[derive(SimpleObject)]
pub struct SavedRepositoryObject {
    pub id: String,
    pub full_name: String,
    pub categories: Vec<String>,
    pub note: Option<String>,
    pub created_at: String,
}

impl TryFrom<Bookmark> for SavedRepositoryObject {
    type Error = AppError;

    fn try_from(value: Bookmark) -> Result<Self, Self::Error> {
        let BookmarkTarget::GitHubRepository { full_name } = value.target else {
            return Err(AppError::Validation(
                "saved target is not a repository".to_string(),
            ));
        };
        Ok(Self {
            id: value.id,
            full_name,
            categories: value.categories,
            note: value.note,
            created_at: value.created_at.to_rfc3339(),
        })
    }
}

fn request_user_id<'a>(context: &'a Context<'_>) -> GraphQlResult<&'a str> {
    context
        .data::<GraphQlSession>()?
        .user_id
        .as_deref()
        .ok_or_else(|| {
            Error::new("authentication required").extend_with(|_, extensions| {
                extensions.set("code", "UNAUTHENTICATED");
            })
        })
}

fn repository_limit(value: i32) -> GraphQlResult<usize> {
    if !(1..=100).contains(&value) {
        return Err(Error::new(
            "repository discovery limit must be between 1 and 100",
        ));
    }
    Ok(value as usize)
}

fn insight_limit(value: i32) -> GraphQlResult<usize> {
    if !(1..=100).contains(&value) {
        return Err(Error::new("insight result limit must be between 1 and 100"));
    }
    Ok(value as usize)
}

fn saturating_i32(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn graphql_error(error: AppError) -> Error {
    let code = match &error {
        AppError::NotFound(_) => "NOT_FOUND",
        AppError::Validation(_) => "BAD_USER_INPUT",
        AppError::Config(_) => "CONFIGURATION_ERROR",
        AppError::External(_) => "EXTERNAL_SERVICE_ERROR",
        AppError::Storage(_) | AppError::Io(_) | AppError::Serde(_) => "STORAGE_ERROR",
        AppError::Unsupported(_) => "UNSUPPORTED",
        AppError::RateBudgetReserved { .. } => "RATE_BUDGET_RESERVED",
        AppError::GraphCapacityExceeded { .. } => "GRAPH_CAPACITY_EXCEEDED",
    };
    let rate_budget = match &error {
        AppError::RateBudgetReserved {
            remaining,
            reserve,
            requested_cost,
            reset_at,
            ..
        } => Some((
            i64::try_from(*remaining).unwrap_or(i64::MAX),
            i64::try_from(*reserve).unwrap_or(i64::MAX),
            i64::try_from(*requested_cost).unwrap_or(i64::MAX),
            reset_at.to_rfc3339(),
        )),
        _ => None,
    };
    let graph_capacity = match &error {
        AppError::GraphCapacityExceeded {
            resource,
            current_count,
            incoming_count,
            projected_count,
            maximum_count,
        } => Some((
            resource.clone(),
            i64::try_from(*current_count).unwrap_or(i64::MAX),
            i64::try_from(*incoming_count).unwrap_or(i64::MAX),
            i64::try_from(*projected_count).unwrap_or(i64::MAX),
            i64::try_from(*maximum_count).unwrap_or(i64::MAX),
        )),
        _ => None,
    };
    Error::new(error.to_string()).extend_with(move |_, extensions| {
        extensions.set("code", code);
        if let Some((remaining, reserve, requested_cost, reset_at)) = rate_budget.as_ref() {
            extensions.set("remaining", *remaining);
            extensions.set("reserve", *reserve);
            extensions.set("requestedCost", *requested_cost);
            extensions.set("resetAt", reset_at.clone());
        }
        if let Some((resource, current, incoming, projected, maximum)) = graph_capacity.as_ref() {
            extensions.set("capacityResource", resource.clone());
            extensions.set("currentCount", *current);
            extensions.set("incomingCount", *incoming);
            extensions.set("projectedCount", *projected);
            extensions.set("maximumCount", *maximum);
        }
    })
}

#[cfg(test)]
mod tests {
    use async_graphql::{EmptySubscription, Request, Schema, Value};

    use super::{GraphQlSession, MutationRoot, QueryRoot, graphql_error};
    use crate::shared::AppError;

    #[test]
    fn schema_exposes_graph_navigation_and_save_operations() {
        let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription).finish();
        let sdl = schema.sdl();

        assert!(sdl.contains("neighborhood(login: String!, limit: Int!)"));
        assert!(sdl.contains("expandUser(login: String!, limit: Int!)"));
        assert!(sdl.contains("saveRepository("));
        assert!(sdl.contains("type UserNeighborhoodObject"));
        assert!(sdl.contains("rateLimit: GitHubRateLimitObject!"));
        assert!(sdl.contains("discoveryWarmup: DiscoveryWarmupObject"));
        assert!(sdl.contains("startDiscoveryWarmup: DiscoveryWarmupObject!"));
        assert!(sdl.contains("enum DiscoveryWarmupStatusObject"));
        assert!(sdl.contains("repositoryInsights(fullName: String!, limit: Int!)"));
        assert!(sdl.contains("userInsights(login: String!, limit: Int!)"));
    }

    #[test]
    fn rate_budget_errors_include_reset_aware_graphql_extensions() {
        let reset_at = chrono::Utc::now() + chrono::Duration::minutes(30);
        let error = graphql_error(AppError::RateBudgetReserved {
            operation: "repository contributor refresh".to_string(),
            remaining: 1_000,
            reserve: 1_000,
            requested_cost: 1,
            reset_at,
        });
        let extensions = error.extensions.expect("rate-budget extensions");

        assert_eq!(
            extensions.get("code"),
            Some(&Value::from("RATE_BUDGET_RESERVED"))
        );
        assert_eq!(extensions.get("remaining"), Some(&Value::from(1_000_i64)));
        assert_eq!(extensions.get("reserve"), Some(&Value::from(1_000_i64)));
        assert_eq!(extensions.get("requestedCost"), Some(&Value::from(1_i64)));
        assert_eq!(
            extensions.get("resetAt"),
            Some(&Value::from(reset_at.to_rfc3339()))
        );
    }

    #[test]
    fn graph_capacity_errors_include_projected_count_extensions() {
        let error = graphql_error(AppError::GraphCapacityExceeded {
            resource: "relationships".to_string(),
            current_count: 379_500,
            incoming_count: 600,
            projected_count: 380_100,
            maximum_count: 380_000,
        });
        let extensions = error.extensions.expect("capacity extensions");

        assert_eq!(
            extensions.get("code"),
            Some(&Value::from("GRAPH_CAPACITY_EXCEEDED"))
        );
        assert_eq!(
            extensions.get("capacityResource"),
            Some(&Value::from("relationships"))
        );
        assert_eq!(
            extensions.get("currentCount"),
            Some(&Value::from(379_500_i64))
        );
        assert_eq!(extensions.get("incomingCount"), Some(&Value::from(600_i64)));
        assert_eq!(
            extensions.get("projectedCount"),
            Some(&Value::from(380_100_i64))
        );
        assert_eq!(
            extensions.get("maximumCount"),
            Some(&Value::from(380_000_i64))
        );
    }

    #[tokio::test]
    async fn private_graph_queries_require_a_cookie_session() {
        let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription).finish();
        let response = schema
            .execute(
                Request::new(r#"{ neighborhood(login: "octocat", limit: 12) { cacheStatus } }"#)
                    .data(GraphQlSession { user_id: None }),
            )
            .await;

        assert_eq!(response.errors.len(), 1);
        assert_eq!(response.errors[0].message, "authentication required");
    }
}
