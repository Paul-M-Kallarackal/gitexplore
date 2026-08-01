use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalUser {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSession {
    pub id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedAccount {
    pub github_user_id: i64,
    pub login: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConnection {
    pub account: ConnectedAccount,
    pub access_token: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSessionResult {
    pub verification_uri: Option<String>,
    pub user_code: Option<String>,
    pub connected_account: Option<ConnectedAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub authenticated: bool,
    pub app_user_id: Option<String>,
    pub connected: bool,
    pub account: Option<ConnectedAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingBrowserLogin {
    pub user_id: String,
    pub redirect_to: Option<String>,
    pub browser_nonce: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CompletedBrowserLogin {
    pub result: AuthSessionResult,
    pub session_id: String,
    pub redirect_to: Option<String>,
}
