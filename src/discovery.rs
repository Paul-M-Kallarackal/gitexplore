use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::graph::{CacheStatus, GitHubRepositoryNode, GitHubUserNode, GraphImportCoverage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryUser {
    pub profile: GitHubUserNode,
    pub neighborhood_cache_status: CacheStatus,
    pub neighborhood_last_fetched_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryRepositoryRecord {
    pub repository: GitHubRepositoryNode,
    pub saved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNeighborhood {
    pub user: DiscoveryUser,
    pub followers: Vec<DiscoveryUser>,
    pub following: Vec<DiscoveryUser>,
    pub starred_repositories: Vec<DiscoveryRepositoryRecord>,
    pub owned_repositories: Vec<DiscoveryRepositoryRecord>,
    pub coverage: GraphImportCoverage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryGraphSignals {
    pub recommenders: usize,
    pub followed_recommenders: usize,
    pub follower_recommenders: usize,
    pub starred_by_recommenders: usize,
    pub owned_by_recommenders: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryReasonKind {
    NetworkEndorsement,
    FollowedDeveloper,
    FollowerDiscovery,
    StarredByNetwork,
    BuiltByNetwork,
    GlobalStars,
    ForkAdoption,
    LanguageMatch,
    RecentActivity,
    HiddenGem,
    ArchivedPenalty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryRankingReason {
    pub kind: RepositoryReasonKind,
    pub description: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryCandidate {
    pub repository: DiscoveryRepositoryRecord,
    pub score: f64,
    pub graph_signals: RepositoryGraphSignals,
    pub via_logins: Vec<String>,
    pub reasons: Vec<RepositoryRankingReason>,
}

pub fn rank_repository_candidate(
    repository: GitHubRepositoryNode,
    saved: bool,
    graph_signals: RepositoryGraphSignals,
    mut via_logins: Vec<String>,
    preferred_languages: &HashSet<String>,
    now: DateTime<Utc>,
) -> RepositoryCandidate {
    let mut reasons = Vec::new();

    push_reason(
        &mut reasons,
        RepositoryReasonKind::NetworkEndorsement,
        graph_signals.recommenders as f64 * 4.0,
        format!(
            "{} distinct network connection(s) point to this repository",
            graph_signals.recommenders
        ),
    );
    push_reason(
        &mut reasons,
        RepositoryReasonKind::FollowedDeveloper,
        graph_signals.followed_recommenders as f64 * 2.0,
        format!(
            "{} account(s) you follow connect to it",
            graph_signals.followed_recommenders
        ),
    );
    push_reason(
        &mut reasons,
        RepositoryReasonKind::FollowerDiscovery,
        graph_signals.follower_recommenders as f64,
        format!(
            "{} follower(s) connect to it",
            graph_signals.follower_recommenders
        ),
    );
    push_reason(
        &mut reasons,
        RepositoryReasonKind::StarredByNetwork,
        graph_signals.starred_by_recommenders as f64 * 1.5,
        format!(
            "{} network account(s) starred it",
            graph_signals.starred_by_recommenders
        ),
    );
    push_reason(
        &mut reasons,
        RepositoryReasonKind::BuiltByNetwork,
        graph_signals.owned_by_recommenders as f64,
        format!(
            "{} network account(s) own it",
            graph_signals.owned_by_recommenders
        ),
    );

    let stars_score = ((repository.stargazer_count as f64) + 1.0).log10() * 1.25;
    push_reason(
        &mut reasons,
        RepositoryReasonKind::GlobalStars,
        stars_score,
        format!(
            "{} global GitHub star(s) provide a quality signal",
            repository.stargazer_count
        ),
    );

    let forks_score = ((repository.fork_count as f64) + 1.0).log10() * 0.75;
    push_reason(
        &mut reasons,
        RepositoryReasonKind::ForkAdoption,
        forks_score,
        format!(
            "{} fork(s) indicate real-world adoption",
            repository.fork_count
        ),
    );

    if let Some(language) = repository.language.as_ref() {
        let normalized = language.to_ascii_lowercase();
        if preferred_languages.contains(&normalized) {
            push_reason(
                &mut reasons,
                RepositoryReasonKind::LanguageMatch,
                2.5,
                format!("{language} matches languages already present in the seed graph"),
            );
        }
    }

    if let Some(activity_at) = repository
        .pushed_at
        .as_ref()
        .or(repository.updated_at.as_ref())
    {
        let age_days = now.signed_duration_since(*activity_at).num_days().max(0);
        let activity_score = if age_days <= 30 {
            3.0
        } else if age_days <= 180 {
            1.75
        } else if age_days <= 365 {
            0.75
        } else {
            0.0
        };
        push_reason(
            &mut reasons,
            RepositoryReasonKind::RecentActivity,
            activity_score,
            format!("repository activity is {age_days} day(s) old"),
        );
    }

    if repository.stargazer_count <= 5_000 && graph_signals.recommenders >= 2 {
        push_reason(
            &mut reasons,
            RepositoryReasonKind::HiddenGem,
            2.0,
            "strong network support with a comparatively small global audience".to_string(),
        );
    }

    if repository.archived {
        reasons.push(RepositoryRankingReason {
            kind: RepositoryReasonKind::ArchivedPenalty,
            description: "archived repositories are ranked lower".to_string(),
            score: -8.0,
        });
    }

    let score = reasons.iter().map(|reason| reason.score).sum::<f64>();
    via_logins.sort();
    via_logins.dedup();
    RepositoryCandidate {
        repository: DiscoveryRepositoryRecord { repository, saved },
        score: (score * 100.0).round() / 100.0,
        graph_signals,
        via_logins,
        reasons,
    }
}

fn push_reason(
    reasons: &mut Vec<RepositoryRankingReason>,
    kind: RepositoryReasonKind,
    score: f64,
    description: String,
) {
    if score > 0.0 {
        reasons.push(RepositoryRankingReason {
            kind,
            description,
            score: (score * 100.0).round() / 100.0,
        });
    }
}
