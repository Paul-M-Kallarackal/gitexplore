use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const CURRENT_ONBOARDING_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStatus {
    NotStarted,
    InProgress,
    Completed,
    Dismissed,
}

impl OnboardingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Dismissed => "dismissed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnboardingRecord {
    pub version: i32,
    pub status: OnboardingStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub dismissed_at: Option<DateTime<Utc>>,
}

impl OnboardingRecord {
    pub fn in_progress(now: DateTime<Utc>) -> Self {
        Self {
            version: CURRENT_ONBOARDING_VERSION,
            status: OnboardingStatus::InProgress,
            started_at: Some(now),
            completed_at: None,
            dismissed_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnboardingProgress {
    pub version: i32,
    pub status: OnboardingStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub dismissed_at: Option<DateTime<Utc>>,
    pub opened_trailhead: bool,
    pub followed_connection: bool,
    pub saved_repository: bool,
    pub mapping_started: bool,
}

impl OnboardingProgress {
    pub fn not_started() -> Self {
        Self {
            version: CURRENT_ONBOARDING_VERSION,
            status: OnboardingStatus::NotStarted,
            started_at: None,
            completed_at: None,
            dismissed_at: None,
            opened_trailhead: false,
            followed_connection: false,
            saved_repository: false,
            mapping_started: false,
        }
    }

    pub fn required_steps_complete(&self) -> bool {
        self.opened_trailhead && self.followed_connection && self.saved_repository
    }
}
