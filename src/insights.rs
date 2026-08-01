use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::graph::{CacheMetadata, CacheStatus};

pub const REPOSITORY_CONTRIBUTOR_CACHE_HOURS: i64 = 24;
pub const USER_COMMIT_ACTIVITY_CACHE_HOURS: i64 = 1;
pub const INSIGHT_REFRESH_RETRY_MINUTES: i64 = 5;
pub const INSIGHT_REFRESH_TIMEOUT_MINUTES: i64 = 5;
pub const USER_COMMIT_ACTIVITY_WINDOW_DAYS: i32 = 30;
pub const USER_COMMIT_ACTIVITY_EVENT_LIMIT: usize = 300;
pub const REPOSITORY_CONTRIBUTOR_LIMIT: usize = 100;

pub const REPOSITORY_CONTRIBUTOR_SOURCE: &str = "GitHub repository contributors, ranked by attributed commits across repository history; GitHub may cache these totals for several hours.";
pub const USER_COMMIT_ACTIVITY_SOURCE: &str = "GitHub public PushEvent activity from the rolling last 30 days, limited by GitHub to the newest 300 public events and subject to event-feed latency.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryContributor {
    pub github_id: i64,
    pub login: String,
    pub avatar_url: Option<String>,
    pub url: String,
    pub contributions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryContributorsSnapshot {
    pub contributors: Vec<RepositoryContributor>,
    pub source_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryContributorInsights {
    pub full_name: String,
    pub contributors: Vec<RepositoryContributor>,
    pub source_complete: bool,
    pub cache: CacheMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserCommitRepository {
    pub github_id: i64,
    pub full_name: String,
    pub url: String,
    pub push_count: u64,
    pub commit_count: u64,
    pub last_pushed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserCommitRepositoriesSnapshot {
    pub repositories: Vec<UserCommitRepository>,
    pub source_event_count: usize,
    pub source_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCommitRepositoryInsights {
    pub login: String,
    pub repositories: Vec<UserCommitRepository>,
    pub source_event_count: usize,
    pub source_truncated: bool,
    pub cache: CacheMetadata,
}

impl RepositoryContributorInsights {
    pub fn from_snapshot(
        full_name: String,
        snapshot: RepositoryContributorsSnapshot,
        fetched_at: DateTime<Utc>,
    ) -> Self {
        Self {
            full_name,
            contributors: snapshot.contributors,
            source_complete: snapshot.source_complete,
            cache: fresh_cache(
                fetched_at,
                fetched_at + Duration::hours(REPOSITORY_CONTRIBUTOR_CACHE_HOURS),
            ),
        }
    }

    pub fn cache_status(&self, now: DateTime<Utc>) -> CacheStatus {
        insight_cache_status(&self.cache, now)
    }

    pub fn limited(mut self, limit: usize) -> Self {
        self.contributors.truncate(limit);
        self
    }
}

impl UserCommitRepositoryInsights {
    pub fn from_snapshot(
        login: String,
        snapshot: UserCommitRepositoriesSnapshot,
        fetched_at: DateTime<Utc>,
    ) -> Self {
        Self {
            login,
            repositories: snapshot.repositories,
            source_event_count: snapshot.source_event_count,
            source_truncated: snapshot.source_truncated,
            cache: fresh_cache(
                fetched_at,
                fetched_at + Duration::hours(USER_COMMIT_ACTIVITY_CACHE_HOURS),
            ),
        }
    }

    pub fn cache_status(&self, now: DateTime<Utc>) -> CacheStatus {
        insight_cache_status(&self.cache, now)
    }

    pub fn limited(mut self, limit: usize) -> Self {
        self.repositories.truncate(limit);
        self
    }
}

pub fn refresh_is_active(metadata: &CacheMetadata, now: DateTime<Utc>) -> bool {
    metadata.refresh_started_at.is_some_and(|started_at| {
        started_at > now - Duration::minutes(INSIGHT_REFRESH_TIMEOUT_MINUTES)
    })
}

fn insight_cache_status(metadata: &CacheMetadata, now: DateTime<Utc>) -> CacheStatus {
    if metadata.last_fetched_at.is_none() {
        CacheStatus::Stale
    } else if refresh_is_active(metadata, now) {
        CacheStatus::Refreshing
    } else if metadata.last_refresh_error.is_some() {
        CacheStatus::RefreshFailed
    } else if metadata.stale_at.is_none_or(|stale_at| stale_at <= now) {
        CacheStatus::Stale
    } else {
        CacheStatus::Fresh
    }
}

fn fresh_cache(fetched_at: DateTime<Utc>, stale_at: DateTime<Utc>) -> CacheMetadata {
    CacheMetadata {
        last_fetched_at: Some(fetched_at),
        stale_at: Some(stale_at),
        refresh_started_at: None,
        last_refresh_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_insight_data_remains_available_while_refreshing() {
        let now = Utc::now();
        let mut insights = RepositoryContributorInsights {
            full_name: "acme/tool".to_string(),
            contributors: vec![RepositoryContributor {
                github_id: 1,
                login: "alice".to_string(),
                avatar_url: None,
                url: "https://github.com/alice".to_string(),
                contributions: 42,
            }],
            source_complete: true,
            cache: CacheMetadata {
                last_fetched_at: Some(now - Duration::days(2)),
                stale_at: Some(now - Duration::days(1)),
                refresh_started_at: None,
                last_refresh_error: None,
            },
        };

        assert_eq!(insights.cache_status(now), CacheStatus::Stale);
        insights.cache.refresh_started_at = Some(now);
        assert_eq!(insights.cache_status(now), CacheStatus::Refreshing);
        assert_eq!(insights.contributors[0].contributions, 42);
    }

    #[test]
    fn abandoned_refresh_is_treated_as_stale() {
        let now = Utc::now();
        let insights = RepositoryContributorInsights {
            full_name: "acme/tool".to_string(),
            contributors: Vec::new(),
            source_complete: true,
            cache: CacheMetadata {
                last_fetched_at: Some(now - Duration::days(2)),
                stale_at: Some(now - Duration::days(1)),
                refresh_started_at: Some(now - Duration::minutes(6)),
                last_refresh_error: None,
            },
        };

        assert_eq!(insights.cache_status(now), CacheStatus::Stale);
    }
}
