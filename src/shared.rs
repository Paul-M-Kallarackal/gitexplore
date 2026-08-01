use std::{fmt::Display, sync::Arc};

use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;
pub type Shared<T> = Arc<T>;
pub const GITHUB_CORE_REST_MINIMUM_RESERVE: usize = 1_000;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("external service error: {0}")]
    External(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error(
        "GitHub REST reserve is active: {remaining} core requests remain; {operation} needs up to {requested_cost}, so GitExplore is preserving {reserve} until {reset_at}"
    )]
    RateBudgetReserved {
        operation: String,
        remaining: usize,
        reserve: usize,
        requested_cost: usize,
        reset_at: DateTime<Utc>,
    },
    #[error(
        "Neo4j capacity gate rejected graph import: {current_count} existing {resource} + up to {incoming_count} incoming = {projected_count}, exceeding the configured maximum of {maximum_count}"
    )]
    GraphCapacityExceeded {
        resource: String,
        current_count: usize,
        incoming_count: usize,
        projected_count: usize,
        maximum_count: usize,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelope {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_cost: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projected_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_count: Option<usize>,
}

impl From<AppError> for ErrorEnvelope {
    fn from(value: AppError) -> Self {
        let mut envelope = Self::message(value.to_string());
        match &value {
            AppError::RateBudgetReserved {
                remaining,
                reserve,
                requested_cost,
                reset_at,
                ..
            } => {
                envelope.code = Some("RATE_BUDGET_RESERVED".to_string());
                envelope.remaining = Some(*remaining);
                envelope.reserve = Some(*reserve);
                envelope.requested_cost = Some(*requested_cost);
                envelope.reset_at = Some(*reset_at);
            }
            AppError::GraphCapacityExceeded {
                resource,
                current_count,
                incoming_count,
                projected_count,
                maximum_count,
            } => {
                envelope.code = Some("GRAPH_CAPACITY_EXCEEDED".to_string());
                envelope.capacity_resource = Some(resource.clone());
                envelope.current_count = Some(*current_count);
                envelope.incoming_count = Some(*incoming_count);
                envelope.projected_count = Some(*projected_count);
                envelope.maximum_count = Some(*maximum_count);
            }
            _ => {}
        }
        envelope
    }
}

impl ErrorEnvelope {
    pub fn message(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: None,
            remaining: None,
            reserve: None,
            requested_cost: None,
            reset_at: None,
            capacity_resource: None,
            current_count: None,
            incoming_count: None,
            projected_count: None,
            maximum_count: None,
        }
    }
}

pub fn ensure(condition: bool, message: impl Display) -> AppResult<()> {
    if condition {
        Ok(())
    } else {
        Err(AppError::Validation(message.to_string()))
    }
}
