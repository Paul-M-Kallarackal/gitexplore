use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration as StdDuration,
};

use async_trait::async_trait;
use axum::http::header::ACCEPT;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use chrono::{DateTime, Duration, Utc};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use neo4rs::{Graph, Row, Txn, query};
use octocrab::{Page, auth::DeviceCodes};
use secrecy::{ExposeSecret, SecretBox, SecretString, zeroize::Zeroize};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;
use uuid::Uuid;

use crate::{
    bookmarks::{Bookmark, BookmarkTarget, Category},
    config::{Neo4jConfig, read_json_file, write_json_file},
    discovery::{
        DiscoveryRepositoryRecord, DiscoveryUser, RepositoryCandidate, RepositoryGraphSignals,
        UserNeighborhood, rank_repository_candidate,
    },
    exploration::{ExplorationResult, ExplorationSeed, ExplorationSnapshot},
    graph::{
        CacheMetadata, CacheStatus, GitHubRateLimitLease, GitHubRateLimitStatus,
        GitHubRepositoryNode, GitHubUserNode, GraphImport, GraphImportCoverage, RefreshJobStatus,
        RefreshLease, RefreshLeaseAttempt, RefreshLeaseState, RefreshLeaseStatus, SyncStatus,
        SyncSummary, UserRefreshOutcome,
    },
    identity::{ConnectedAccount, GitHubConnection, PendingBrowserLogin},
    insights::{
        INSIGHT_REFRESH_RETRY_MINUTES, INSIGHT_REFRESH_TIMEOUT_MINUTES,
        REPOSITORY_CONTRIBUTOR_LIMIT, RepositoryContributor, RepositoryContributorInsights,
        RepositoryContributorsSnapshot, USER_COMMIT_ACTIVITY_EVENT_LIMIT,
        UserCommitRepositoriesSnapshot, UserCommitRepository, UserCommitRepositoryInsights,
        refresh_is_active,
    },
    ports::{
        BookmarkRepository, CategoryRepository, DeviceLoginStart, DiscoveryRepository,
        ExplorationRepository, GitHubAuthConfig, GitHubClientPort, GitHubImportRepository,
        IdentityRepository, InsightRepository, SyncStateRepository,
    },
    shared::{AppError, AppResult, GITHUB_CORE_REST_MINIMUM_RESERVE},
};

const IDENTITY_CIPHERTEXT_VERSION: &str = "v1";
const IDENTITY_KEY_BYTES: usize = 32;
const GITHUB_TOKEN_PURPOSE: &str = "github-access-token";
const BROWSER_NONCE_PURPOSE: &str = "oauth-browser-nonce";

struct IdentityCipher {
    encryption_key: SecretBox<[u8; IDENTITY_KEY_BYTES]>,
    digest_key: SecretBox<[u8; IDENTITY_KEY_BYTES]>,
}

impl IdentityCipher {
    fn from_secret(encoded_key: &SecretString) -> AppResult<Self> {
        let mut decoded = URL_SAFE_NO_PAD
            .decode(encoded_key.expose_secret())
            .map_err(|_| {
                AppError::Config(
                    "GITEXPLORE_IDENTITY_ENCRYPTION_KEY must be unpadded base64url encoding of exactly 32 random bytes"
                        .to_string(),
                )
            })?;
        if decoded.len() != IDENTITY_KEY_BYTES {
            decoded.zeroize();
            return Err(AppError::Config(
                "GITEXPLORE_IDENTITY_ENCRYPTION_KEY must be unpadded base64url encoding of exactly 32 random bytes"
                    .to_string(),
            ));
        }
        let mut master_key = [0_u8; IDENTITY_KEY_BYTES];
        master_key.copy_from_slice(&decoded);
        decoded.zeroize();
        let hkdf = Hkdf::<Sha256>::new(Some(b"gitexplore identity v1"), &master_key);
        let mut encryption_key = [0_u8; IDENTITY_KEY_BYTES];
        let mut digest_key = [0_u8; IDENTITY_KEY_BYTES];
        hkdf.expand(b"xchacha20poly1305 encryption", &mut encryption_key)
            .map_err(|_| {
                AppError::Config("identity encryption key derivation failed".to_string())
            })?;
        hkdf.expand(b"hmac-sha256 opaque ids", &mut digest_key)
            .map_err(|_| AppError::Config("identity digest key derivation failed".to_string()))?;
        master_key.zeroize();
        Ok(Self {
            encryption_key: SecretBox::new(Box::new(encryption_key)),
            digest_key: SecretBox::new(Box::new(digest_key)),
        })
    }

    fn encrypt(&self, purpose: &str, context: &str, plaintext: &str) -> AppResult<String> {
        let cipher = XChaCha20Poly1305::new_from_slice(self.encryption_key.expose_secret())
            .map_err(|_| AppError::Storage("identity cipher initialization failed".to_string()))?;
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: identity_aad(purpose, context).as_bytes(),
                },
            )
            .map_err(|_| AppError::Storage("identity secret encryption failed".to_string()))?;
        Ok(format!(
            "{IDENTITY_CIPHERTEXT_VERSION}.{}.{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    fn decrypt(&self, purpose: &str, context: &str, encoded: &str) -> AppResult<String> {
        let mut parts = encoded.split('.');
        let version = parts.next();
        let nonce = parts.next();
        let ciphertext = parts.next();
        if version != Some(IDENTITY_CIPHERTEXT_VERSION)
            || nonce.is_none()
            || ciphertext.is_none()
            || parts.next().is_some()
        {
            return Err(AppError::Storage(
                "identity secret has an unsupported ciphertext format".to_string(),
            ));
        }
        let nonce = URL_SAFE_NO_PAD
            .decode(nonce.expect("nonce checked above"))
            .map_err(|_| AppError::Storage("identity secret nonce is malformed".to_string()))?;
        if nonce.len() != 24 {
            return Err(AppError::Storage(
                "identity secret nonce is malformed".to_string(),
            ));
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext.expect("ciphertext checked above"))
            .map_err(|_| {
                AppError::Storage("identity secret ciphertext is malformed".to_string())
            })?;
        let nonce: [u8; 24] = nonce
            .try_into()
            .map_err(|_| AppError::Storage("identity secret nonce is malformed".to_string()))?;
        let nonce = XNonce::from(nonce);
        let cipher = XChaCha20Poly1305::new_from_slice(self.encryption_key.expose_secret())
            .map_err(|_| AppError::Storage("identity cipher initialization failed".to_string()))?;
        let plaintext = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &ciphertext,
                    aad: identity_aad(purpose, context).as_bytes(),
                },
            )
            .map_err(|_| {
                AppError::Storage(
                    "identity secret authentication failed; verify the configured encryption key"
                        .to_string(),
                )
            })?;
        String::from_utf8(plaintext)
            .map_err(|_| AppError::Storage("identity secret is not valid UTF-8".to_string()))
    }

    fn digest(&self, purpose: &str, value: &str) -> AppResult<String> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(self.digest_key.expose_secret())
            .map_err(|_| AppError::Storage("identity digest initialization failed".to_string()))?;
        mac.update(identity_aad("opaque-id", purpose).as_bytes());
        mac.update(&[0]);
        mac.update(value.as_bytes());
        Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }
}

fn identity_aad(purpose: &str, context: &str) -> String {
    format!("gitexplore:{IDENTITY_CIPHERTEXT_VERSION}:{purpose}:{context}")
}

pub struct LocalRepositorySet {
    pub identity: Arc<dyn IdentityRepository>,
    pub imports: Arc<dyn GitHubImportRepository>,
    pub sync_state: Arc<dyn SyncStateRepository>,
    pub categories: Arc<dyn CategoryRepository>,
    pub bookmarks: Arc<dyn BookmarkRepository>,
    pub exploration: Arc<dyn ExplorationRepository>,
    pub discovery: Arc<dyn DiscoveryRepository>,
    pub insights: Arc<dyn InsightRepository>,
}

impl LocalRepositorySet {
    pub fn in_memory() -> Self {
        let graph_store = Arc::new(LocalGraphStore::in_memory());
        Self::from_parts(Arc::new(InMemoryIdentityRepository::default()), graph_store)
    }

    pub fn from_files(
        identity_path: PathBuf,
        graph_path: PathBuf,
        identity_encryption_key: &SecretString,
    ) -> AppResult<Self> {
        let identity = Arc::new(JsonIdentityRepository::new(
            identity_path,
            identity_encryption_key,
        )?);
        let graph_store = Arc::new(LocalGraphStore::from_file(graph_path)?);
        Ok(Self::from_parts(identity, graph_store))
    }

    fn from_parts(
        identity: Arc<dyn IdentityRepository>,
        graph_store: Arc<LocalGraphStore>,
    ) -> Self {
        Self {
            identity,
            imports: Arc::new(LocalGitHubImportRepository {
                store: graph_store.clone(),
            }),
            sync_state: Arc::new(LocalSyncStateRepository {
                store: graph_store.clone(),
            }),
            categories: Arc::new(LocalCategoryRepository {
                store: graph_store.clone(),
            }),
            bookmarks: Arc::new(LocalBookmarkRepository {
                store: graph_store.clone(),
            }),
            exploration: Arc::new(LocalExplorationRepository {
                store: graph_store.clone(),
            }),
            discovery: Arc::new(LocalDiscoveryRepository {
                store: graph_store.clone(),
            }),
            insights: Arc::new(LocalInsightRepository { store: graph_store }),
        }
    }
}

pub struct Neo4jRepositorySet {
    pub identity: Arc<dyn IdentityRepository>,
    pub imports: Arc<dyn GitHubImportRepository>,
    pub sync_state: Arc<dyn SyncStateRepository>,
    pub categories: Arc<dyn CategoryRepository>,
    pub bookmarks: Arc<dyn BookmarkRepository>,
    pub exploration: Arc<dyn ExplorationRepository>,
    pub discovery: Arc<dyn DiscoveryRepository>,
    pub insights: Arc<dyn InsightRepository>,
}

impl Neo4jRepositorySet {
    pub async fn new(
        config: &Neo4jConfig,
        identity_encryption_key: &SecretString,
    ) -> AppResult<Self> {
        let client = Arc::new(Neo4jClient::new(config).await?);
        crate::schema::check_neo4j_schema_client(&client).await?;
        let cipher = Arc::new(IdentityCipher::from_secret(identity_encryption_key)?);
        Ok(Self {
            identity: Arc::new(Neo4jIdentityRepository {
                client: client.clone(),
                cipher,
            }),
            imports: Arc::new(Neo4jGitHubImportRepository {
                client: client.clone(),
                max_total_nodes: config.max_total_nodes,
                max_total_relationships: config.max_total_relationships,
            }),
            sync_state: Arc::new(Neo4jSyncStateRepository {
                client: client.clone(),
            }),
            categories: Arc::new(Neo4jCategoryRepository {
                client: client.clone(),
            }),
            bookmarks: Arc::new(Neo4jBookmarkRepository {
                client: client.clone(),
            }),
            exploration: Arc::new(Neo4jExplorationRepository {
                client: client.clone(),
            }),
            discovery: Arc::new(Neo4jDiscoveryRepository {
                client: client.clone(),
            }),
            insights: Arc::new(Neo4jInsightRepository { client }),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityMigrationSummary {
    pub account_links: usize,
    pub connections: usize,
    pub rate_limits: usize,
    pub pending_browser_logins: usize,
    pub sessions: usize,
    pub migrated_at: DateTime<Utc>,
}

pub async fn migrate_json_identity_to_neo4j(
    identity_path: PathBuf,
    neo4j_config: &Neo4jConfig,
    identity_encryption_key: &SecretString,
) -> AppResult<IdentityMigrationSummary> {
    // Opening with a cipher upgrades a legacy plaintext file atomically before any
    // network write, so migration never sends or leaves a plaintext token at rest.
    let source = JsonIdentityRepository::new(identity_path, identity_encryption_key)?;
    let snapshot = source
        .state
        .lock()
        .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
        .clone();
    if let Some(migrated_at) = snapshot.neo4j_migrated_at {
        return Err(AppError::Validation(format!(
            "identity.json was already migrated to Neo4j at {}",
            migrated_at.to_rfc3339()
        )));
    }

    let client = Arc::new(Neo4jClient::new(neo4j_config).await?);
    let target = Neo4jIdentityRepository {
        client,
        cipher: Arc::new(IdentityCipher::from_secret(identity_encryption_key)?),
    };

    for (github_user_id, user_id) in &snapshot.account_links {
        let github_user_id = github_user_id.parse::<i64>().map_err(|_| {
            AppError::Storage(format!(
                "identity.json contains invalid GitHub account id `{github_user_id}`"
            ))
        })?;
        target.import_account_link(user_id, github_user_id).await?;
    }

    for (user_id, stored_connection) in &snapshot.connections {
        let connection = source.decode_connection(stored_connection.clone())?;
        target.save_connection(user_id, connection).await?;
    }

    for (github_user_id, status) in &snapshot.github_rate_limits {
        let github_user_id = github_user_id.parse::<i64>().map_err(|_| {
            AppError::Storage(format!(
                "identity.json contains invalid GitHub rate-limit account id `{github_user_id}`"
            ))
        })?;
        target
            .save_github_rate_limit(github_user_id, status.clone())
            .await?;
    }

    for (state_id, stored_pending) in &snapshot.pending_browser_logins {
        let pending = source.decode_pending(state_id, stored_pending.clone())?;
        if pending.expires_at > Utc::now() {
            target.save_pending_browser_login(state_id, pending).await?;
        }
    }

    let mut session_count = 0;
    for (session_id, stored_session) in &snapshot.sessions {
        if let StoredSession::Current(session) = stored_session
            && session.expires_at > Utc::now()
        {
            target
                .import_session(session_id, &session.user_id, session.expires_at)
                .await?;
            session_count += 1;
        }
    }

    let migrated_at = Utc::now();
    {
        let mut guard = source
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard.neo4j_migrated_at = Some(migrated_at);
        source.persist(&guard)?;
    }

    Ok(IdentityMigrationSummary {
        account_links: snapshot.account_links.len(),
        connections: snapshot.connections.len(),
        rate_limits: snapshot.github_rate_limits.len(),
        pending_browser_logins: snapshot
            .pending_browser_logins
            .values()
            .filter(|pending| pending.expires_at > Utc::now())
            .count(),
        sessions: session_count,
        migrated_at,
    })
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct IdentityStore {
    connections: HashMap<String, GitHubConnection>,
    account_links: HashMap<String, String>,
    github_rate_limits: HashMap<String, GitHubRateLimitStatus>,
    github_rate_limit_leases: HashMap<String, StoredGitHubRateLimitLease>,
    pending_browser_logins: HashMap<String, PendingBrowserLogin>,
    sessions: HashMap<String, StoredSession>,
    neo4j_migrated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum StoredSession {
    Current(SessionRecord),
    Legacy(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionRecord {
    user_id: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredGitHubRateLimitLease {
    token: String,
    expires_at: DateTime<Utc>,
}

const SESSION_TTL_DAYS: i64 = 30;
const MAX_ACTIVE_SESSIONS: usize = 4_096;
const MAX_PENDING_BROWSER_LOGINS: usize = 256;

impl IdentityStore {
    fn normalize_legacy_data(&mut self) -> bool {
        let mut changed = false;
        for (user_id, connection) in &self.connections {
            let github_user_id = connection.account.github_user_id.to_string();
            if let std::collections::hash_map::Entry::Vacant(entry) =
                self.account_links.entry(github_user_id)
            {
                entry.insert(user_id.clone());
                changed = true;
            }
        }
        let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);
        for session in self.sessions.values_mut() {
            if let StoredSession::Legacy(user_id) = session {
                *session = StoredSession::Current(SessionRecord {
                    user_id: user_id.clone(),
                    expires_at,
                });
                changed = true;
            }
        }
        changed |= self.purge_expired_sessions();
        changed |= self.purge_expired_browser_logins();
        changed |= self.trim_pending_browser_logins_to(MAX_PENDING_BROWSER_LOGINS);
        changed |= self.trim_sessions_to(MAX_ACTIVE_SESSIONS);
        changed |= self.purge_expired_github_rate_limit_leases();
        changed
    }

    fn canonical_browser_user(
        &mut self,
        preferred_user_id: &str,
        connection: &GitHubConnection,
    ) -> String {
        let github_user_id = connection.account.github_user_id.to_string();
        if let Some(user_id) = self.account_links.get(&github_user_id) {
            return user_id.clone();
        }
        let preferred_is_linked_elsewhere =
            self.account_links
                .iter()
                .any(|(existing_github_id, user_id)| {
                    user_id == preferred_user_id && existing_github_id != &github_user_id
                });
        let preferred_has_other_connection =
            self.connections
                .get(preferred_user_id)
                .is_some_and(|existing| {
                    existing.account.github_user_id != connection.account.github_user_id
                });
        let canonical_user_id = if preferred_is_linked_elsewhere || preferred_has_other_connection {
            Uuid::new_v4().to_string()
        } else {
            preferred_user_id.to_string()
        };
        self.account_links
            .insert(github_user_id, canonical_user_id.clone());
        canonical_user_id
    }

    fn create_session(&mut self, session_id: &str, user_id: &str) {
        self.purge_expired_sessions();
        self.sessions.remove(session_id);
        self.trim_sessions_to(MAX_ACTIVE_SESSIONS.saturating_sub(1));
        self.sessions.insert(
            session_id.to_string(),
            StoredSession::Current(SessionRecord {
                user_id: user_id.to_string(),
                expires_at: Utc::now() + Duration::days(SESSION_TTL_DAYS),
            }),
        );
    }

    fn save_pending_browser_login(&mut self, state_id: &str, pending: PendingBrowserLogin) {
        self.purge_expired_browser_logins();
        self.pending_browser_logins.remove(state_id);
        self.trim_pending_browser_logins_to(MAX_PENDING_BROWSER_LOGINS.saturating_sub(1));
        self.pending_browser_logins
            .insert(state_id.to_string(), pending);
    }

    fn purge_expired_browser_logins(&mut self) -> bool {
        let previous_len = self.pending_browser_logins.len();
        let now = Utc::now();
        self.pending_browser_logins
            .retain(|_, pending| pending.expires_at > now);
        self.pending_browser_logins.len() != previous_len
    }

    fn trim_pending_browser_logins_to(&mut self, limit: usize) -> bool {
        if self.pending_browser_logins.len() <= limit {
            return false;
        }
        let excess = self.pending_browser_logins.len() - limit;
        let mut states_by_creation = self
            .pending_browser_logins
            .iter()
            .map(|(state_id, pending)| (state_id.clone(), pending.created_at))
            .collect::<Vec<_>>();
        states_by_creation.sort_by_key(|(_, created_at)| *created_at);
        for (state_id, _) in states_by_creation.into_iter().take(excess) {
            self.pending_browser_logins.remove(&state_id);
        }
        true
    }

    fn purge_expired_sessions(&mut self) -> bool {
        let previous_len = self.sessions.len();
        let now = Utc::now();
        self.sessions.retain(|_, session| match session {
            StoredSession::Current(record) => record.expires_at > now,
            StoredSession::Legacy(_) => false,
        });
        self.sessions.len() != previous_len
    }

    fn trim_sessions_to(&mut self, limit: usize) -> bool {
        if self.sessions.len() <= limit {
            return false;
        }
        let excess = self.sessions.len() - limit;
        let mut sessions_by_expiry = self
            .sessions
            .iter()
            .map(|(id, session)| {
                let expires_at = match session {
                    StoredSession::Current(record) => Some(record.expires_at),
                    StoredSession::Legacy(_) => None,
                };
                (id.clone(), expires_at)
            })
            .collect::<Vec<_>>();
        sessions_by_expiry.sort_by_key(|(_, expires_at)| *expires_at);
        for (id, _) in sessions_by_expiry.into_iter().take(excess) {
            self.sessions.remove(&id);
        }
        true
    }

    fn session_user(&self, session_id: &str) -> Option<String> {
        match self.sessions.get(session_id) {
            Some(StoredSession::Current(record)) => Some(record.user_id.clone()),
            Some(StoredSession::Legacy(user_id)) => Some(user_id.clone()),
            None => None,
        }
    }

    fn purge_expired_github_rate_limit_leases(&mut self) -> bool {
        let previous_len = self.github_rate_limit_leases.len();
        let now = Utc::now();
        self.github_rate_limit_leases
            .retain(|_, lease| lease.expires_at > now);
        self.github_rate_limit_leases.len() != previous_len
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct SharedGraphStore {
    users: HashMap<String, GitHubUserNode>,
    repositories: HashMap<String, GitHubRepositoryNode>,
    follows: HashSet<(String, String)>,
    starred: HashSet<(String, String)>,
    member_of: HashSet<(String, String)>,
    #[serde(default)]
    user_coverage: HashMap<String, GraphImportCoverage>,
    user_cache: HashMap<String, CacheMetadata>,
    repository_cache: HashMap<String, CacheMetadata>,
    repository_contributor_insights: HashMap<String, RepositoryContributorInsights>,
    user_commit_repository_insights: HashMap<String, UserCommitRepositoryInsights>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct GraphStore {
    sync_status: HashMap<String, SyncStatus>,
    shared: SharedGraphStore,
    categories: HashMap<String, Vec<Category>>,
    bookmarks: HashMap<String, Vec<Bookmark>>,
    snapshots: HashMap<String, Vec<ExplorationSnapshot>>,
}

pub struct JsonIdentityRepository {
    path: PathBuf,
    state: Mutex<IdentityStore>,
    cipher: Arc<IdentityCipher>,
}

impl JsonIdentityRepository {
    pub fn new(path: PathBuf, identity_encryption_key: &SecretString) -> AppResult<Self> {
        let cipher = Arc::new(IdentityCipher::from_secret(identity_encryption_key)?);
        let mut state: IdentityStore = read_json_file(&path)?;
        let mut migrated = state.normalize_legacy_data();
        migrated |= encrypt_legacy_json_identity_secrets(&mut state, &cipher)?;
        let repository = Self {
            path,
            state: Mutex::new(state),
            cipher,
        };
        if migrated {
            let guard = repository
                .state
                .lock()
                .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
            repository.persist(&guard)?;
        }
        Ok(repository)
    }

    fn persist(&self, state: &IdentityStore) -> AppResult<()> {
        write_json_file(&self.path, state)
    }

    fn encode_connection(&self, mut connection: GitHubConnection) -> AppResult<GitHubConnection> {
        connection.access_token = self.cipher.encrypt(
            GITHUB_TOKEN_PURPOSE,
            &connection.account.github_user_id.to_string(),
            &connection.access_token,
        )?;
        Ok(connection)
    }

    fn decode_connection(&self, mut connection: GitHubConnection) -> AppResult<GitHubConnection> {
        if connection
            .access_token
            .starts_with(&format!("{IDENTITY_CIPHERTEXT_VERSION}."))
        {
            connection.access_token = self.cipher.decrypt(
                GITHUB_TOKEN_PURPOSE,
                &connection.account.github_user_id.to_string(),
                &connection.access_token,
            )?;
        } else {
            return Err(AppError::Storage(
                "identity connection token is not encrypted".to_string(),
            ));
        }
        Ok(connection)
    }

    fn encode_pending(
        &self,
        state_id: &str,
        mut pending: PendingBrowserLogin,
    ) -> AppResult<PendingBrowserLogin> {
        pending.browser_nonce =
            self.cipher
                .encrypt(BROWSER_NONCE_PURPOSE, state_id, &pending.browser_nonce)?;
        Ok(pending)
    }

    fn decode_pending(
        &self,
        state_id: &str,
        mut pending: PendingBrowserLogin,
    ) -> AppResult<PendingBrowserLogin> {
        if pending
            .browser_nonce
            .starts_with(&format!("{IDENTITY_CIPHERTEXT_VERSION}."))
        {
            pending.browser_nonce =
                self.cipher
                    .decrypt(BROWSER_NONCE_PURPOSE, state_id, &pending.browser_nonce)?;
        } else {
            return Err(AppError::Storage(
                "OAuth browser nonce is not encrypted".to_string(),
            ));
        }
        Ok(pending)
    }
}

fn encrypt_legacy_json_identity_secrets(
    state: &mut IdentityStore,
    cipher: &IdentityCipher,
) -> AppResult<bool> {
    let mut changed = false;
    for connection in state.connections.values_mut() {
        if connection
            .access_token
            .starts_with(&format!("{IDENTITY_CIPHERTEXT_VERSION}."))
        {
            cipher.decrypt(
                GITHUB_TOKEN_PURPOSE,
                &connection.account.github_user_id.to_string(),
                &connection.access_token,
            )?;
        } else {
            connection.access_token = cipher.encrypt(
                GITHUB_TOKEN_PURPOSE,
                &connection.account.github_user_id.to_string(),
                &connection.access_token,
            )?;
            changed = true;
        }
    }
    for (state_id, pending) in &mut state.pending_browser_logins {
        if pending
            .browser_nonce
            .starts_with(&format!("{IDENTITY_CIPHERTEXT_VERSION}."))
        {
            cipher.decrypt(BROWSER_NONCE_PURPOSE, state_id, &pending.browser_nonce)?;
        } else {
            pending.browser_nonce =
                cipher.encrypt(BROWSER_NONCE_PURPOSE, state_id, &pending.browser_nonce)?;
            changed = true;
        }
    }
    Ok(changed)
}

#[async_trait]
impl IdentityRepository for JsonIdentityRepository {
    async fn get_connection(&self, user_id: &str) -> AppResult<Option<GitHubConnection>> {
        let connection = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .connections
            .get(user_id)
            .cloned();
        connection
            .map(|connection| self.decode_connection(connection))
            .transpose()
    }

    async fn save_connection(&self, user_id: &str, connection: GitHubConnection) -> AppResult<()> {
        let connection = self.encode_connection(connection)?;
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard
            .account_links
            .entry(connection.account.github_user_id.to_string())
            .or_insert_with(|| user_id.to_string());
        guard.connections.insert(user_id.to_string(), connection);
        self.persist(&guard)
    }

    async fn save_browser_connection(
        &self,
        preferred_user_id: &str,
        connection: GitHubConnection,
    ) -> AppResult<String> {
        let connection = self.encode_connection(connection)?;
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let canonical_user_id = guard.canonical_browser_user(preferred_user_id, &connection);
        guard
            .connections
            .insert(canonical_user_id.clone(), connection);
        self.persist(&guard)?;
        Ok(canonical_user_id)
    }

    async fn clear_connection(&self, user_id: &str) -> AppResult<()> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard.connections.remove(user_id);
        self.persist(&guard)
    }

    async fn save_pending_browser_login(
        &self,
        state_id: &str,
        pending: PendingBrowserLogin,
    ) -> AppResult<()> {
        let pending = self.encode_pending(state_id, pending)?;
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard.save_pending_browser_login(state_id, pending);
        self.persist(&guard)
    }

    async fn consume_pending_browser_login(
        &self,
        state_id: &str,
    ) -> AppResult<Option<PendingBrowserLogin>> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let pending = guard.pending_browser_logins.remove(state_id);
        let purged = guard.purge_expired_browser_logins();
        if pending.is_some() || purged {
            self.persist(&guard)?;
        }
        drop(guard);
        pending
            .map(|pending| self.decode_pending(state_id, pending))
            .transpose()
    }

    async fn create_session(&self, session_id: &str, user_id: &str) -> AppResult<()> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard.create_session(session_id, user_id);
        self.persist(&guard)
    }

    async fn get_user_id_for_session(&self, session_id: &str) -> AppResult<Option<String>> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let purged = guard.purge_expired_sessions();
        let user_id = guard.session_user(session_id);
        if purged {
            self.persist(&guard)?;
        }
        Ok(user_id)
    }

    async fn clear_session(&self, session_id: &str) -> AppResult<()> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard.sessions.remove(session_id);
        self.persist(&guard)
    }

    async fn github_rate_limit(
        &self,
        github_user_id: i64,
    ) -> AppResult<Option<GitHubRateLimitStatus>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .github_rate_limits
            .get(&github_user_id.to_string())
            .cloned())
    }

    async fn save_github_rate_limit(
        &self,
        github_user_id: i64,
        status: GitHubRateLimitStatus,
    ) -> AppResult<()> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard
            .github_rate_limits
            .insert(github_user_id.to_string(), status);
        self.persist(&guard)
    }

    async fn try_acquire_github_rate_limit_lease(
        &self,
        github_user_id: i64,
        token: &str,
        lease_seconds: i64,
    ) -> AppResult<Option<GitHubRateLimitLease>> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let key = github_user_id.to_string();
        let now = Utc::now();
        if guard
            .github_rate_limit_leases
            .get(&key)
            .is_some_and(|lease| lease.expires_at > now)
        {
            return Ok(None);
        }
        let expires_at = now + Duration::seconds(lease_seconds);
        guard.github_rate_limit_leases.insert(
            key,
            StoredGitHubRateLimitLease {
                token: token.to_string(),
                expires_at,
            },
        );
        self.persist(&guard)?;
        Ok(Some(GitHubRateLimitLease {
            github_user_id,
            token: token.to_string(),
            expires_at,
        }))
    }

    async fn renew_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
        lease_seconds: i64,
    ) -> AppResult<bool> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let key = lease.github_user_id.to_string();
        let now = Utc::now();
        let Some(current) = guard.github_rate_limit_leases.get_mut(&key) else {
            return Ok(false);
        };
        if current.token != lease.token || current.expires_at <= now {
            return Ok(false);
        }
        current.expires_at = now + Duration::seconds(lease_seconds);
        self.persist(&guard)?;
        Ok(true)
    }

    async fn release_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
    ) -> AppResult<bool> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let key = lease.github_user_id.to_string();
        if guard
            .github_rate_limit_leases
            .get(&key)
            .is_none_or(|current| current.token != lease.token)
        {
            return Ok(false);
        }
        guard.github_rate_limit_leases.remove(&key);
        self.persist(&guard)?;
        Ok(true)
    }
}

#[derive(Default)]
pub struct InMemoryIdentityRepository {
    state: Mutex<IdentityStore>,
}

#[async_trait]
impl IdentityRepository for InMemoryIdentityRepository {
    async fn get_connection(&self, user_id: &str) -> AppResult<Option<GitHubConnection>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .connections
            .get(user_id)
            .cloned())
    }

    async fn save_connection(&self, user_id: &str, connection: GitHubConnection) -> AppResult<()> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard
            .account_links
            .entry(connection.account.github_user_id.to_string())
            .or_insert_with(|| user_id.to_string());
        guard.connections.insert(user_id.to_string(), connection);
        Ok(())
    }

    async fn save_browser_connection(
        &self,
        preferred_user_id: &str,
        connection: GitHubConnection,
    ) -> AppResult<String> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let canonical_user_id = guard.canonical_browser_user(preferred_user_id, &connection);
        guard
            .connections
            .insert(canonical_user_id.clone(), connection);
        Ok(canonical_user_id)
    }

    async fn clear_connection(&self, user_id: &str) -> AppResult<()> {
        self.state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .connections
            .remove(user_id);
        Ok(())
    }

    async fn save_pending_browser_login(
        &self,
        state_id: &str,
        pending: PendingBrowserLogin,
    ) -> AppResult<()> {
        self.state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .save_pending_browser_login(state_id, pending);
        Ok(())
    }

    async fn consume_pending_browser_login(
        &self,
        state_id: &str,
    ) -> AppResult<Option<PendingBrowserLogin>> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let pending = guard.pending_browser_logins.remove(state_id);
        guard.purge_expired_browser_logins();
        Ok(pending)
    }

    async fn create_session(&self, session_id: &str, user_id: &str) -> AppResult<()> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard.create_session(session_id, user_id);
        Ok(())
    }

    async fn get_user_id_for_session(&self, session_id: &str) -> AppResult<Option<String>> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        guard.purge_expired_sessions();
        Ok(guard.session_user(session_id))
    }

    async fn clear_session(&self, session_id: &str) -> AppResult<()> {
        self.state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .sessions
            .remove(session_id);
        Ok(())
    }

    async fn github_rate_limit(
        &self,
        github_user_id: i64,
    ) -> AppResult<Option<GitHubRateLimitStatus>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .github_rate_limits
            .get(&github_user_id.to_string())
            .cloned())
    }

    async fn save_github_rate_limit(
        &self,
        github_user_id: i64,
        status: GitHubRateLimitStatus,
    ) -> AppResult<()> {
        self.state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?
            .github_rate_limits
            .insert(github_user_id.to_string(), status);
        Ok(())
    }

    async fn try_acquire_github_rate_limit_lease(
        &self,
        github_user_id: i64,
        token: &str,
        lease_seconds: i64,
    ) -> AppResult<Option<GitHubRateLimitLease>> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let key = github_user_id.to_string();
        let now = Utc::now();
        if guard
            .github_rate_limit_leases
            .get(&key)
            .is_some_and(|lease| lease.expires_at > now)
        {
            return Ok(None);
        }
        let expires_at = now + Duration::seconds(lease_seconds);
        guard.github_rate_limit_leases.insert(
            key,
            StoredGitHubRateLimitLease {
                token: token.to_string(),
                expires_at,
            },
        );
        Ok(Some(GitHubRateLimitLease {
            github_user_id,
            token: token.to_string(),
            expires_at,
        }))
    }

    async fn renew_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
        lease_seconds: i64,
    ) -> AppResult<bool> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let key = lease.github_user_id.to_string();
        let now = Utc::now();
        let Some(current) = guard.github_rate_limit_leases.get_mut(&key) else {
            return Ok(false);
        };
        if current.token != lease.token || current.expires_at <= now {
            return Ok(false);
        }
        current.expires_at = now + Duration::seconds(lease_seconds);
        Ok(true)
    }

    async fn release_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
    ) -> AppResult<bool> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| AppError::Storage("identity store lock poisoned".to_string()))?;
        let key = lease.github_user_id.to_string();
        if guard
            .github_rate_limit_leases
            .get(&key)
            .is_none_or(|current| current.token != lease.token)
        {
            return Ok(false);
        }
        guard.github_rate_limit_leases.remove(&key);
        Ok(true)
    }
}

struct LocalGraphStore {
    path: Option<PathBuf>,
    state: Mutex<GraphStore>,
}

impl LocalGraphStore {
    fn in_memory() -> Self {
        Self {
            path: None,
            state: Mutex::new(GraphStore::default()),
        }
    }

    fn from_file(path: PathBuf) -> AppResult<Self> {
        let state = read_json_file(&path)?;
        Ok(Self {
            path: Some(path),
            state: Mutex::new(state),
        })
    }

    fn lock(&self) -> AppResult<MutexGuard<'_, GraphStore>> {
        self.state
            .lock()
            .map_err(|_| AppError::Storage("graph store lock poisoned".to_string()))
    }

    fn persist(&self, state: &GraphStore) -> AppResult<()> {
        if let Some(path) = &self.path {
            write_json_file(path, state)?;
        }
        Ok(())
    }
}

struct LocalGitHubImportRepository {
    store: Arc<LocalGraphStore>,
}

struct LocalSyncStateRepository {
    store: Arc<LocalGraphStore>,
}

struct LocalCategoryRepository {
    store: Arc<LocalGraphStore>,
}

struct LocalBookmarkRepository {
    store: Arc<LocalGraphStore>,
}

struct LocalExplorationRepository {
    store: Arc<LocalGraphStore>,
}

struct LocalDiscoveryRepository {
    store: Arc<LocalGraphStore>,
}

struct LocalInsightRepository {
    store: Arc<LocalGraphStore>,
}

#[async_trait]
impl GitHubImportRepository for LocalGitHubImportRepository {
    async fn import_github_graph(
        &self,
        _user_id: &str,
        import: GraphImport,
    ) -> AppResult<SyncSummary> {
        let fetched_at = Utc::now();
        let stale_at = fetched_at + Duration::hours(6);
        let summary = SyncSummary {
            followers: import.followers.len(),
            following: import.following.len(),
            starred_repositories: import.starred_repositories.len(),
            repositories: import.repositories.len(),
            synced_at: fetched_at,
            coverage: import.coverage,
        };
        let mut guard = self.store.lock()?;
        upsert_import(&mut guard, import, fetched_at, stale_at);
        self.store.persist(&guard)?;
        Ok(summary)
    }

    async fn resolve_bookmark_target(&self, target: &BookmarkTarget) -> AppResult<()> {
        let guard = self.store.lock()?;
        resolve_target_in_store(&guard.shared, target)
    }
}

#[async_trait]
impl SyncStateRepository for LocalSyncStateRepository {
    async fn sync_status(&self, user_id: &str) -> AppResult<SyncStatus> {
        Ok(self
            .store
            .lock()?
            .sync_status
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn set_sync_status(&self, user_id: &str, status: SyncStatus) -> AppResult<()> {
        let mut guard = self.store.lock()?;
        guard.sync_status.insert(user_id.to_string(), status);
        self.store.persist(&guard)
    }
}

#[async_trait]
impl CategoryRepository for LocalCategoryRepository {
    async fn create_category(&self, user_id: &str, category: Category) -> AppResult<()> {
        let mut guard = self.store.lock()?;
        let categories = guard.categories.entry(user_id.to_string()).or_default();
        if !categories
            .iter()
            .any(|existing| existing.name == category.name)
        {
            categories.push(category);
        }
        self.store.persist(&guard)
    }

    async fn list_categories(&self, user_id: &str) -> AppResult<Vec<Category>> {
        Ok(self
            .store
            .lock()?
            .categories
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }
}

#[async_trait]
impl BookmarkRepository for LocalBookmarkRepository {
    async fn add_bookmark(&self, user_id: &str, bookmark: Bookmark) -> AppResult<Bookmark> {
        let mut guard = self.store.lock()?;
        let bookmarks = guard.bookmarks.entry(user_id.to_string()).or_default();
        if let Some(existing) = bookmarks
            .iter()
            .find(|existing| bookmark_targets_match(&existing.target, &bookmark.target))
            .cloned()
        {
            return Ok(existing);
        }
        bookmarks.push(bookmark.clone());
        self.store.persist(&guard)?;
        Ok(bookmark)
    }

    async fn list_bookmarks(&self, user_id: &str) -> AppResult<Vec<Bookmark>> {
        Ok(self
            .store
            .lock()?
            .bookmarks
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }
}

fn bookmark_targets_match(left: &BookmarkTarget, right: &BookmarkTarget) -> bool {
    match (left, right) {
        (
            BookmarkTarget::GitHubUser { login: left },
            BookmarkTarget::GitHubUser { login: right },
        ) => left.eq_ignore_ascii_case(right),
        (
            BookmarkTarget::GitHubRepository { full_name: left },
            BookmarkTarget::GitHubRepository { full_name: right },
        ) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}

#[async_trait]
impl ExplorationRepository for LocalExplorationRepository {
    async fn explore(&self, user_id: &str, seed: ExplorationSeed) -> AppResult<ExplorationResult> {
        let mut guard = self.store.lock()?;
        let bookmarks = guard.bookmarks.get(user_id).cloned().unwrap_or_default();

        let mut related_people = HashSet::new();
        let mut related_repositories = HashSet::new();
        let (cache_status, last_fetched_at, refresh_job_status) = match &seed {
            ExplorationSeed::User { login } => {
                if !guard.shared.users.contains_key(login) {
                    return Err(AppError::Validation(
                        "run sync before exploring".to_string(),
                    ));
                }
                for (from, to) in &guard.shared.follows {
                    if from == login {
                        related_people.insert(to.clone());
                    }
                    if to == login {
                        related_people.insert(from.clone());
                    }
                }
                for (graph_user, repo) in guard
                    .shared
                    .member_of
                    .iter()
                    .chain(guard.shared.starred.iter())
                {
                    if graph_user == login {
                        related_repositories.insert(repo.clone());
                    }
                }
                cache_status_from_metadata(
                    guard
                        .shared
                        .user_cache
                        .get(login)
                        .cloned()
                        .unwrap_or_default(),
                )
            }
            ExplorationSeed::Repository { full_name } => {
                if !guard.shared.repositories.contains_key(full_name) {
                    return Err(AppError::Validation(
                        "run sync before exploring".to_string(),
                    ));
                }
                let mut connected_users = HashSet::new();
                for (graph_user, repo) in guard
                    .shared
                    .member_of
                    .iter()
                    .chain(guard.shared.starred.iter())
                {
                    if repo == full_name {
                        connected_users.insert(graph_user.clone());
                        related_people.insert(graph_user.clone());
                    }
                }
                for connected_user in connected_users {
                    for (graph_user, repo) in guard
                        .shared
                        .member_of
                        .iter()
                        .chain(guard.shared.starred.iter())
                    {
                        if graph_user == &connected_user && repo != full_name {
                            related_repositories.insert(repo.clone());
                        }
                    }
                }
                cache_status_from_metadata(
                    guard
                        .shared
                        .repository_cache
                        .get(full_name)
                        .cloned()
                        .unwrap_or_default(),
                )
            }
            ExplorationSeed::Category { name } => {
                for bookmark in bookmarks
                    .iter()
                    .filter(|bookmark| bookmark.categories.iter().any(|category| category == name))
                {
                    match &bookmark.target {
                        BookmarkTarget::GitHubUser { login } => {
                            related_people.insert(login.clone());
                        }
                        BookmarkTarget::GitHubRepository { full_name } => {
                            related_repositories.insert(full_name.clone());
                        }
                    }
                }
                (CacheStatus::Fresh, None, None)
            }
        };

        let snapshot = ExplorationSnapshot {
            id: Uuid::new_v4().to_string(),
            seed: seed.clone(),
            discovered_people: related_people.iter().cloned().collect(),
            discovered_repositories: related_repositories.iter().cloned().collect(),
            generated_at: Utc::now(),
        };
        guard
            .snapshots
            .entry(user_id.to_string())
            .or_default()
            .push(snapshot.clone());
        self.store.persist(&guard)?;

        Ok(ExplorationResult {
            seed,
            related_people: snapshot.discovered_people.clone(),
            related_repositories: snapshot.discovered_repositories.clone(),
            saved_snapshot: snapshot,
            cache_status,
            last_fetched_at,
            refresh_job_status,
            overload_message: None,
        })
    }

    async fn list_exploration_snapshots(
        &self,
        user_id: &str,
    ) -> AppResult<Vec<ExplorationSnapshot>> {
        Ok(self
            .store
            .lock()?
            .snapshots
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }
}

#[async_trait]
impl DiscoveryRepository for LocalDiscoveryRepository {
    async fn user_neighborhood(&self, user_id: &str, login: &str) -> AppResult<UserNeighborhood> {
        let guard = self.store.lock()?;
        let shared = &guard.shared;
        let requested_login = login;
        let login = resolve_shared_login(shared, login).ok_or_else(|| {
            AppError::NotFound(format!(
                "github user `{requested_login}` is not present in the shared graph"
            ))
        })?;

        let saved = saved_repository_names(guard.bookmarks.get(user_id));
        let mut followers = shared
            .follows
            .iter()
            .filter(|(_, to)| to == &login)
            .filter_map(|(from, _)| discovery_user_from_store(shared, from))
            .collect::<Vec<_>>();
        let mut following = shared
            .follows
            .iter()
            .filter(|(from, _)| from == &login)
            .filter_map(|(_, to)| discovery_user_from_store(shared, to))
            .collect::<Vec<_>>();
        followers.sort_by(|left, right| left.profile.login.cmp(&right.profile.login));
        following.sort_by(|left, right| left.profile.login.cmp(&right.profile.login));

        let mut starred_repositories = shared
            .starred
            .iter()
            .filter(|(graph_user, _)| graph_user == &login)
            .filter_map(|(_, full_name)| discovery_repository_from_store(shared, full_name, &saved))
            .collect::<Vec<_>>();
        let mut owned_repositories = shared
            .member_of
            .iter()
            .filter(|(graph_user, _)| graph_user == &login)
            .filter_map(|(_, full_name)| discovery_repository_from_store(shared, full_name, &saved))
            .collect::<Vec<_>>();
        sort_repository_records(&mut starred_repositories);
        sort_repository_records(&mut owned_repositories);

        let coverage = shared
            .user_coverage
            .get(&login)
            .copied()
            .unwrap_or_else(GraphImportCoverage::incomplete);
        let mut user = discovery_user_from_store(shared, &login).expect("user existence checked");
        if !coverage.is_complete() {
            user.neighborhood_cache_status = CacheStatus::Stale;
        }

        Ok(UserNeighborhood {
            user,
            followers,
            following,
            starred_repositories,
            owned_repositories,
            coverage,
        })
    }

    async fn discover_repositories(
        &self,
        user_id: &str,
        login: &str,
        limit: usize,
    ) -> AppResult<Vec<RepositoryCandidate>> {
        let guard = self.store.lock()?;
        let shared = &guard.shared;
        let requested_login = login;
        let login = resolve_shared_login(shared, login).ok_or_else(|| {
            AppError::NotFound(format!(
                "github user `{requested_login}` is not present in the shared graph"
            ))
        })?;

        let seed_repositories = shared
            .starred
            .iter()
            .chain(shared.member_of.iter())
            .filter(|(graph_user, _)| graph_user == &login)
            .map(|(_, full_name)| full_name.clone())
            .collect::<HashSet<_>>();
        let preferred_languages = seed_repositories
            .iter()
            .filter_map(|full_name| shared.repositories.get(full_name))
            .filter_map(|repository| repository.language.as_ref())
            .map(|language| language.to_ascii_lowercase())
            .collect::<HashSet<_>>();

        let followed = shared
            .follows
            .iter()
            .filter(|(from, _)| from == &login)
            .map(|(_, to)| to.clone())
            .collect::<HashSet<_>>();
        let followers = shared
            .follows
            .iter()
            .filter(|(_, to)| to == &login)
            .map(|(from, _)| from.clone())
            .collect::<HashSet<_>>();
        let peers = followed
            .iter()
            .chain(followers.iter())
            .cloned()
            .collect::<HashSet<_>>();

        let mut accumulators = HashMap::<String, CandidateSignalAccumulator>::new();
        for (peer, full_name) in &shared.starred {
            if peers.contains(peer) && !seed_repositories.contains(full_name) {
                accumulators.entry(full_name.clone()).or_default().record(
                    peer,
                    followed.contains(peer),
                    followers.contains(peer),
                    true,
                );
            }
        }
        for (peer, full_name) in &shared.member_of {
            if peers.contains(peer) && !seed_repositories.contains(full_name) {
                accumulators.entry(full_name.clone()).or_default().record(
                    peer,
                    followed.contains(peer),
                    followers.contains(peer),
                    false,
                );
            }
        }

        let saved = saved_repository_names(guard.bookmarks.get(user_id));
        let mut candidates = accumulators
            .into_iter()
            .filter_map(|(full_name, accumulator)| {
                let repository = shared.repositories.get(&full_name)?.clone();
                let (graph_signals, via_logins) = accumulator.into_parts();
                Some(rank_repository_candidate(
                    repository,
                    saved.contains(&full_name),
                    graph_signals,
                    via_logins,
                    &preferred_languages,
                    Utc::now(),
                ))
            })
            .collect::<Vec<_>>();
        sort_repository_candidates(&mut candidates);
        candidates.truncate(limit);
        Ok(candidates)
    }
}

#[async_trait]
impl InsightRepository for LocalInsightRepository {
    async fn repository_contributors(
        &self,
        full_name: &str,
    ) -> AppResult<Option<RepositoryContributorInsights>> {
        let guard = self.store.lock()?;
        let Some(canonical) = resolve_shared_repository_name(&guard.shared, full_name) else {
            return Ok(None);
        };
        Ok(guard
            .shared
            .repository_contributor_insights
            .get(&canonical)
            .cloned())
    }

    async fn user_commit_repositories(
        &self,
        login: &str,
    ) -> AppResult<Option<UserCommitRepositoryInsights>> {
        let guard = self.store.lock()?;
        let Some(canonical) = resolve_shared_login(&guard.shared, login) else {
            return Ok(None);
        };
        Ok(guard
            .shared
            .user_commit_repository_insights
            .get(&canonical)
            .cloned())
    }

    async fn begin_repository_contributor_refresh(&self, full_name: &str) -> AppResult<bool> {
        let mut guard = self.store.lock()?;
        let Some(canonical) = resolve_shared_repository_name(&guard.shared, full_name) else {
            return Ok(false);
        };
        let Some(insights) = guard
            .shared
            .repository_contributor_insights
            .get_mut(&canonical)
        else {
            return Ok(false);
        };
        if !begin_insight_refresh(&mut insights.cache, Utc::now()) {
            return Ok(false);
        }
        self.store.persist(&guard)?;
        Ok(true)
    }

    async fn begin_user_commit_repository_refresh(&self, login: &str) -> AppResult<bool> {
        let mut guard = self.store.lock()?;
        let Some(canonical) = resolve_shared_login(&guard.shared, login) else {
            return Ok(false);
        };
        let Some(insights) = guard
            .shared
            .user_commit_repository_insights
            .get_mut(&canonical)
        else {
            return Ok(false);
        };
        if !begin_insight_refresh(&mut insights.cache, Utc::now()) {
            return Ok(false);
        }
        self.store.persist(&guard)?;
        Ok(true)
    }

    async fn save_repository_contributors(
        &self,
        mut insights: RepositoryContributorInsights,
    ) -> AppResult<()> {
        let mut guard = self.store.lock()?;
        let canonical = resolve_shared_repository_name(&guard.shared, &insights.full_name)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "github repository `{}` is not present in the shared graph",
                    insights.full_name
                ))
            })?;
        insights.full_name = canonical.clone();
        insights.contributors.sort_by(|left, right| {
            right
                .contributions
                .cmp(&left.contributions)
                .then_with(|| left.login.cmp(&right.login))
        });
        let mut user_alias_changes = Vec::new();
        for contributor in &insights.contributors {
            let user = GitHubUserNode {
                github_id: contributor.github_id,
                login: contributor.login.clone(),
                name: None,
                url: contributor.url.clone(),
                avatar_url: contributor.avatar_url.clone(),
                bio: None,
                followers_count: None,
                following_count: None,
                public_repositories_count: None,
            };
            user_alias_changes.extend(reconcile_user_alias(&mut guard.shared, &user));
            upsert_user_record(&mut guard.shared.users, user, false);
        }
        update_bookmark_aliases(&mut guard.bookmarks, &user_alias_changes, &[]);
        guard
            .shared
            .repository_contributor_insights
            .insert(canonical, insights);
        self.store.persist(&guard)
    }

    async fn save_user_commit_repositories(
        &self,
        mut insights: UserCommitRepositoryInsights,
    ) -> AppResult<()> {
        let mut guard = self.store.lock()?;
        let canonical = resolve_shared_login(&guard.shared, &insights.login).ok_or_else(|| {
            AppError::NotFound(format!(
                "github user `{}` is not present in the shared graph",
                insights.login
            ))
        })?;
        insights.login = canonical.clone();
        insights.repositories.sort_by(|left, right| {
            right
                .commit_count
                .cmp(&left.commit_count)
                .then_with(|| right.push_count.cmp(&left.push_count))
                .then_with(|| right.last_pushed_at.cmp(&left.last_pushed_at))
                .then_with(|| left.full_name.cmp(&right.full_name))
        });
        let mut repository_alias_changes = Vec::new();
        for recent in &insights.repositories {
            let (owner_login, name) = recent.full_name.split_once('/').ok_or_else(|| {
                AppError::External(format!(
                    "GitHub event returned invalid repository name `{}`",
                    recent.full_name
                ))
            })?;
            let repository = GitHubRepositoryNode {
                github_id: recent.github_id,
                owner_login: owner_login.to_string(),
                name: name.to_string(),
                full_name: recent.full_name.clone(),
                description: None,
                html_url: recent.url.clone(),
                stargazer_count: 0,
                fork_count: 0,
                language: None,
                topics: Vec::new(),
                pushed_at: Some(recent.last_pushed_at),
                updated_at: None,
                archived: false,
                is_fork: false,
            };
            repository_alias_changes
                .extend(reconcile_repository_alias(&mut guard.shared, &repository));
            upsert_partial_repository_record(&mut guard.shared.repositories, repository);
        }
        update_bookmark_aliases(&mut guard.bookmarks, &[], &repository_alias_changes);
        guard
            .shared
            .user_commit_repository_insights
            .insert(canonical, insights);
        self.store.persist(&guard)
    }

    async fn fail_repository_contributor_refresh(
        &self,
        full_name: &str,
        error: &str,
    ) -> AppResult<()> {
        let mut guard = self.store.lock()?;
        if let Some(canonical) = resolve_shared_repository_name(&guard.shared, full_name)
            && let Some(insights) = guard
                .shared
                .repository_contributor_insights
                .get_mut(&canonical)
        {
            fail_insight_refresh(&mut insights.cache, error, Utc::now());
            self.store.persist(&guard)?;
        }
        Ok(())
    }

    async fn fail_user_commit_repository_refresh(&self, login: &str, error: &str) -> AppResult<()> {
        let mut guard = self.store.lock()?;
        if let Some(canonical) = resolve_shared_login(&guard.shared, login)
            && let Some(insights) = guard
                .shared
                .user_commit_repository_insights
                .get_mut(&canonical)
        {
            fail_insight_refresh(&mut insights.cache, error, Utc::now());
            self.store.persist(&guard)?;
        }
        Ok(())
    }
}

fn begin_insight_refresh(metadata: &mut CacheMetadata, now: DateTime<Utc>) -> bool {
    if refresh_is_active(metadata, now)
        || metadata
            .stale_at
            .is_some_and(|retry_at| metadata.last_refresh_error.is_some() && retry_at > now)
    {
        return false;
    }
    metadata.refresh_started_at = Some(now);
    metadata.last_refresh_error = None;
    true
}

fn fail_insight_refresh(metadata: &mut CacheMetadata, error: &str, now: DateTime<Utc>) {
    metadata.refresh_started_at = None;
    metadata.last_refresh_error = Some(error.to_string());
    metadata.stale_at = Some(now + Duration::minutes(INSIGHT_REFRESH_RETRY_MINUTES));
}

fn discovery_user_from_store(store: &SharedGraphStore, login: &str) -> Option<DiscoveryUser> {
    let profile = store.users.get(login)?.clone();
    let (neighborhood_cache_status, neighborhood_last_fetched_at, _) =
        cache_status_from_metadata(store.user_cache.get(login).cloned().unwrap_or_default());
    Some(DiscoveryUser {
        profile,
        neighborhood_cache_status,
        neighborhood_last_fetched_at,
    })
}

fn resolve_shared_login(store: &SharedGraphStore, requested: &str) -> Option<String> {
    if store.users.contains_key(requested) {
        return Some(requested.to_string());
    }
    store
        .users
        .keys()
        .find(|login| login.eq_ignore_ascii_case(requested))
        .cloned()
}

fn resolve_shared_repository_name(store: &SharedGraphStore, requested: &str) -> Option<String> {
    if store.repositories.contains_key(requested) {
        return Some(requested.to_string());
    }
    store
        .repositories
        .keys()
        .find(|full_name| full_name.eq_ignore_ascii_case(requested))
        .cloned()
}

fn discovery_repository_from_store(
    store: &SharedGraphStore,
    full_name: &str,
    saved: &HashSet<String>,
) -> Option<DiscoveryRepositoryRecord> {
    Some(DiscoveryRepositoryRecord {
        repository: store.repositories.get(full_name)?.clone(),
        saved: saved.contains(full_name),
    })
}

fn saved_repository_names(bookmarks: Option<&Vec<Bookmark>>) -> HashSet<String> {
    bookmarks
        .into_iter()
        .flatten()
        .filter_map(|bookmark| match &bookmark.target {
            BookmarkTarget::GitHubRepository { full_name } => Some(full_name.clone()),
            BookmarkTarget::GitHubUser { .. } => None,
        })
        .collect()
}

#[derive(Default)]
struct CandidateSignalAccumulator {
    recommenders: HashSet<String>,
    followed_recommenders: HashSet<String>,
    follower_recommenders: HashSet<String>,
    starred_by_recommenders: HashSet<String>,
    owned_by_recommenders: HashSet<String>,
}

impl CandidateSignalAccumulator {
    fn record(&mut self, login: &str, followed: bool, follower: bool, starred: bool) {
        self.recommenders.insert(login.to_string());
        if followed {
            self.followed_recommenders.insert(login.to_string());
        }
        if follower {
            self.follower_recommenders.insert(login.to_string());
        }
        if starred {
            self.starred_by_recommenders.insert(login.to_string());
        } else {
            self.owned_by_recommenders.insert(login.to_string());
        }
    }

    fn into_parts(self) -> (RepositoryGraphSignals, Vec<String>) {
        let via_logins = self.recommenders.iter().cloned().collect();
        let signals = RepositoryGraphSignals {
            recommenders: self.recommenders.len(),
            followed_recommenders: self.followed_recommenders.len(),
            follower_recommenders: self.follower_recommenders.len(),
            starred_by_recommenders: self.starred_by_recommenders.len(),
            owned_by_recommenders: self.owned_by_recommenders.len(),
        };
        (signals, via_logins)
    }
}

fn sort_repository_records(records: &mut [DiscoveryRepositoryRecord]) {
    records.sort_by(|left, right| left.repository.full_name.cmp(&right.repository.full_name));
}

fn sort_repository_candidates(candidates: &mut [RepositoryCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| {
                right
                    .repository
                    .repository
                    .stargazer_count
                    .cmp(&left.repository.repository.stargazer_count)
            })
            .then_with(|| {
                left.repository
                    .repository
                    .full_name
                    .cmp(&right.repository.repository.full_name)
            })
    });
}

fn resolve_target_in_store(store: &SharedGraphStore, target: &BookmarkTarget) -> AppResult<()> {
    let found = match target {
        BookmarkTarget::GitHubUser { login } => store
            .users
            .keys()
            .any(|stored| stored.eq_ignore_ascii_case(login)),
        BookmarkTarget::GitHubRepository { full_name } => store
            .repositories
            .keys()
            .any(|stored| stored.eq_ignore_ascii_case(full_name)),
    };

    if found {
        Ok(())
    } else {
        Err(AppError::NotFound(
            "bookmark target not found in imported graph".to_string(),
        ))
    }
}

fn upsert_import(
    store: &mut GraphStore,
    import: GraphImport,
    fetched_at: DateTime<Utc>,
    stale_at: DateTime<Utc>,
) {
    let coverage = import.coverage;
    let shared = &mut store.shared;
    let mut user_alias_changes = Vec::new();
    let mut repository_alias_changes = Vec::new();
    let metadata = CacheMetadata {
        last_fetched_at: Some(fetched_at),
        stale_at: Some(if coverage.is_complete() {
            stale_at
        } else {
            fetched_at
        }),
        refresh_started_at: None,
        last_refresh_error: None,
    };

    if let Some(viewer) = import.viewer {
        let viewer_login = viewer.login.clone();
        user_alias_changes.extend(reconcile_user_alias(shared, &viewer));
        shared.follows.retain(|(from, to)| {
            (!coverage.following_complete || from != &viewer_login)
                && (!coverage.followers_complete || to != &viewer_login)
        });
        if coverage.starred_repositories_complete {
            shared.starred.retain(|(login, _)| login != &viewer_login);
        }
        if coverage.repositories_complete {
            shared.member_of.retain(|(login, _)| login != &viewer_login);
        }

        upsert_user_record(&mut shared.users, viewer, true);
        shared.user_coverage.insert(viewer_login.clone(), coverage);
        shared
            .user_cache
            .insert(viewer_login.clone(), metadata.clone());

        for user in import.followers {
            user_alias_changes.extend(reconcile_user_alias(shared, &user));
            upsert_user_record(&mut shared.users, user.clone(), false);
            shared.follows.insert((user.login, viewer_login.clone()));
        }

        for user in import.following {
            user_alias_changes.extend(reconcile_user_alias(shared, &user));
            upsert_user_record(&mut shared.users, user.clone(), false);
            shared.follows.insert((viewer_login.clone(), user.login));
        }

        for repo in import.repositories {
            repository_alias_changes.extend(reconcile_repository_alias(shared, &repo));
            upsert_repository_record(&mut shared.repositories, repo.clone());
            shared
                .repository_cache
                .insert(repo.full_name.clone(), metadata.clone());
            shared
                .member_of
                .insert((viewer_login.clone(), repo.full_name));
        }

        for repo in import.starred_repositories {
            repository_alias_changes.extend(reconcile_repository_alias(shared, &repo));
            upsert_repository_record(&mut shared.repositories, repo.clone());
            shared
                .repository_cache
                .insert(repo.full_name.clone(), metadata.clone());
            shared
                .starred
                .insert((viewer_login.clone(), repo.full_name));
        }
    }
    update_bookmark_aliases(
        &mut store.bookmarks,
        &user_alias_changes,
        &repository_alias_changes,
    );
}

fn reconcile_user_alias(
    store: &mut SharedGraphStore,
    incoming: &GitHubUserNode,
) -> Vec<(String, String)> {
    let mut changes = Vec::new();
    if let Some((occupied_login, occupied_id)) = store
        .users
        .iter()
        .find(|(login, user)| {
            login.eq_ignore_ascii_case(&incoming.login) && user.github_id != incoming.github_id
        })
        .map(|(login, user)| (login.clone(), user.github_id))
    {
        let replacement = format!("__gitexplore-user-{occupied_id}");
        rename_user_alias(store, &occupied_login, &replacement);
        changes.push((occupied_login, replacement));
    }
    if let Some(previous_login) = store
        .users
        .iter()
        .find(|(login, user)| user.github_id == incoming.github_id && *login != &incoming.login)
        .map(|(login, _)| login.clone())
    {
        rename_user_alias(store, &previous_login, &incoming.login);
        changes.push((previous_login, incoming.login.clone()));
    }
    changes
}

fn rename_user_alias(store: &mut SharedGraphStore, previous: &str, next: &str) {
    if let Some(mut user) = store.users.remove(previous) {
        user.login = next.to_string();
        store.users.insert(next.to_string(), user);
    }
    if let Some(metadata) = store.user_cache.remove(previous) {
        store.user_cache.insert(next.to_string(), metadata);
    }
    if let Some(coverage) = store.user_coverage.remove(previous) {
        store.user_coverage.insert(next.to_string(), coverage);
    }
    if let Some(mut insights) = store.user_commit_repository_insights.remove(previous) {
        insights.login = next.to_string();
        store
            .user_commit_repository_insights
            .insert(next.to_string(), insights);
    }
    for insights in store.repository_contributor_insights.values_mut() {
        for contributor in &mut insights.contributors {
            if contributor.login.eq_ignore_ascii_case(previous) {
                contributor.login = next.to_string();
            }
        }
    }
    store.follows = store
        .follows
        .drain()
        .map(|(from, to)| {
            (
                if from.eq_ignore_ascii_case(previous) {
                    next.to_string()
                } else {
                    from
                },
                if to.eq_ignore_ascii_case(previous) {
                    next.to_string()
                } else {
                    to
                },
            )
        })
        .collect();
    store.starred = store
        .starred
        .drain()
        .map(|(login, repository)| {
            (
                if login.eq_ignore_ascii_case(previous) {
                    next.to_string()
                } else {
                    login
                },
                repository,
            )
        })
        .collect();
    store.member_of = store
        .member_of
        .drain()
        .map(|(login, repository)| {
            (
                if login.eq_ignore_ascii_case(previous) {
                    next.to_string()
                } else {
                    login
                },
                repository,
            )
        })
        .collect();
    for repository in store.repositories.values_mut() {
        if repository.owner_login.eq_ignore_ascii_case(previous) {
            repository.owner_login = next.to_string();
        }
    }
}

fn reconcile_repository_alias(
    store: &mut SharedGraphStore,
    incoming: &GitHubRepositoryNode,
) -> Vec<(String, String)> {
    let mut changes = Vec::new();
    if let Some((occupied_name, occupied_id)) = store
        .repositories
        .iter()
        .find(|(full_name, repository)| {
            full_name.eq_ignore_ascii_case(&incoming.full_name)
                && repository.github_id != incoming.github_id
        })
        .map(|(full_name, repository)| (full_name.clone(), repository.github_id))
    {
        let replacement = format!("__gitexplore-repository-{occupied_id}");
        rename_repository_alias(store, &occupied_name, &replacement);
        changes.push((occupied_name, replacement));
    }
    if let Some(previous_name) = store
        .repositories
        .iter()
        .find(|(full_name, repository)| {
            repository.github_id == incoming.github_id && *full_name != &incoming.full_name
        })
        .map(|(full_name, _)| full_name.clone())
    {
        rename_repository_alias(store, &previous_name, &incoming.full_name);
        changes.push((previous_name, incoming.full_name.clone()));
    }
    changes
}

fn rename_repository_alias(store: &mut SharedGraphStore, previous: &str, next: &str) {
    if let Some(mut repository) = store.repositories.remove(previous) {
        repository.full_name = next.to_string();
        store.repositories.insert(next.to_string(), repository);
    }
    if let Some(metadata) = store.repository_cache.remove(previous) {
        store.repository_cache.insert(next.to_string(), metadata);
    }
    if let Some(mut insights) = store.repository_contributor_insights.remove(previous) {
        insights.full_name = next.to_string();
        store
            .repository_contributor_insights
            .insert(next.to_string(), insights);
    }
    for insights in store.user_commit_repository_insights.values_mut() {
        for repository in &mut insights.repositories {
            if repository.full_name.eq_ignore_ascii_case(previous) {
                repository.full_name = next.to_string();
            }
        }
    }
    store.starred = store
        .starred
        .drain()
        .map(|(login, repository)| {
            (
                login,
                if repository.eq_ignore_ascii_case(previous) {
                    next.to_string()
                } else {
                    repository
                },
            )
        })
        .collect();
    store.member_of = store
        .member_of
        .drain()
        .map(|(login, repository)| {
            (
                login,
                if repository.eq_ignore_ascii_case(previous) {
                    next.to_string()
                } else {
                    repository
                },
            )
        })
        .collect();
}

fn update_bookmark_aliases(
    bookmarks: &mut HashMap<String, Vec<Bookmark>>,
    user_changes: &[(String, String)],
    repository_changes: &[(String, String)],
) {
    for bookmark in bookmarks.values_mut().flatten() {
        match &mut bookmark.target {
            BookmarkTarget::GitHubUser { login } => {
                for (previous, replacement) in user_changes {
                    if login.eq_ignore_ascii_case(previous) {
                        *login = replacement.clone();
                    }
                }
            }
            BookmarkTarget::GitHubRepository { full_name } => {
                for (previous, replacement) in repository_changes {
                    if full_name.eq_ignore_ascii_case(previous) {
                        *full_name = replacement.clone();
                    }
                }
            }
        }
    }
}

fn upsert_user_record(
    users: &mut HashMap<String, GitHubUserNode>,
    incoming: GitHubUserNode,
    authoritative: bool,
) {
    if authoritative {
        users.insert(incoming.login.clone(), incoming);
        return;
    }

    users
        .entry(incoming.login.clone())
        .and_modify(|existing| {
            existing.github_id = incoming.github_id;
            existing.url = incoming.url.clone();
            existing.name = incoming.name.clone().or_else(|| existing.name.clone());
            existing.avatar_url = incoming
                .avatar_url
                .clone()
                .or_else(|| existing.avatar_url.clone());
            existing.bio = incoming.bio.clone().or_else(|| existing.bio.clone());
            existing.followers_count = incoming.followers_count.or(existing.followers_count);
            existing.following_count = incoming.following_count.or(existing.following_count);
            existing.public_repositories_count = incoming
                .public_repositories_count
                .or(existing.public_repositories_count);
        })
        .or_insert(incoming);
}

fn upsert_repository_record(
    repositories: &mut HashMap<String, GitHubRepositoryNode>,
    incoming: GitHubRepositoryNode,
) {
    repositories
        .entry(incoming.full_name.clone())
        .and_modify(|existing| {
            *existing = incoming.clone();
        })
        .or_insert(incoming);
}

fn upsert_partial_repository_record(
    repositories: &mut HashMap<String, GitHubRepositoryNode>,
    incoming: GitHubRepositoryNode,
) {
    repositories
        .entry(incoming.full_name.clone())
        .and_modify(|existing| {
            existing.github_id = incoming.github_id;
            existing.owner_login = incoming.owner_login.clone();
            existing.name = incoming.name.clone();
            if existing.html_url.is_empty() {
                existing.html_url = incoming.html_url.clone();
            }
            existing.pushed_at = existing.pushed_at.max(incoming.pushed_at);
        })
        .or_insert(incoming);
}

fn cache_status_from_metadata(
    metadata: CacheMetadata,
) -> (CacheStatus, Option<DateTime<Utc>>, Option<RefreshJobStatus>) {
    let status = if metadata.last_fetched_at.is_none() {
        CacheStatus::Stale
    } else if metadata.refresh_started_at.is_some() {
        CacheStatus::Refreshing
    } else if metadata.last_refresh_error.is_some() {
        CacheStatus::RefreshFailed
    } else if metadata
        .stale_at
        .map(|value| value <= Utc::now())
        .unwrap_or(false)
    {
        CacheStatus::Stale
    } else {
        CacheStatus::Fresh
    };

    let refresh_job_status = match status {
        CacheStatus::Refreshing => Some(RefreshJobStatus::Running),
        CacheStatus::RefreshFailed => Some(RefreshJobStatus::Failed),
        _ => None,
    };

    (status, metadata.last_fetched_at, refresh_job_status)
}

pub struct OctocrabGitHubClient {
    auth_client: octocrab::Octocrab,
}

const MAX_SOCIAL_CONNECTIONS_PER_EXPANSION: usize = 300;
const MAX_REPOSITORIES_PER_EXPANSION: usize = 300;
const GITHUB_CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(10);
const GITHUB_REQUEST_TIMEOUT: StdDuration = StdDuration::from_secs(30);

struct BoundedCollection<T> {
    items: Vec<T>,
    complete: bool,
}

impl OctocrabGitHubClient {
    pub fn new() -> AppResult<Self> {
        let auth_client = octocrab::Octocrab::builder()
            .base_uri("https://github.com")
            .map_err(|error| AppError::External(error.to_string()))?
            .add_header(ACCEPT, "application/json".to_string())
            .set_connect_timeout(Some(GITHUB_CONNECT_TIMEOUT))
            .set_read_timeout(Some(GITHUB_REQUEST_TIMEOUT))
            .set_write_timeout(Some(GITHUB_REQUEST_TIMEOUT))
            .build()
            .map_err(|error| AppError::External(error.to_string()))?;
        Ok(Self { auth_client })
    }

    fn user_client(token: &str) -> AppResult<octocrab::Octocrab> {
        octocrab::Octocrab::builder()
            .user_access_token(SecretString::from(token.to_string()))
            .set_connect_timeout(Some(GITHUB_CONNECT_TIMEOUT))
            .set_read_timeout(Some(GITHUB_REQUEST_TIMEOUT))
            .set_write_timeout(Some(GITHUB_REQUEST_TIMEOUT))
            .build()
            .map_err(|error| AppError::External(error.to_string()))
    }

    fn oauth_client() -> AppResult<reqwest::Client> {
        reqwest::Client::builder()
            .connect_timeout(GITHUB_CONNECT_TIMEOUT)
            .timeout(GITHUB_REQUEST_TIMEOUT)
            .build()
            .map_err(|error| AppError::External(error.to_string()))
    }

    async fn fetch_core_rate_limit_for_client(
        crab: &octocrab::Octocrab,
    ) -> AppResult<GitHubRateLimitStatus> {
        let core = crab
            .ratelimit()
            .get()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .resources
            .core;
        let reset = i64::try_from(core.reset).map_err(|_| {
            AppError::External("GitHub rate-limit reset is out of range".to_string())
        })?;
        let reset_at = DateTime::<Utc>::from_timestamp(reset, 0).ok_or_else(|| {
            AppError::External("GitHub rate-limit reset is not a valid timestamp".to_string())
        })?;
        Ok(GitHubRateLimitStatus {
            limit: core.limit,
            used: core.used,
            remaining: core.remaining,
            reset_at,
            checked_at: Utc::now(),
        })
    }

    async fn ensure_oauth_identity_budget(crab: &octocrab::Octocrab) -> AppResult<()> {
        let status = Self::fetch_core_rate_limit_for_client(crab).await?;
        Self::ensure_oauth_identity_budget_status(&status)
    }

    fn ensure_oauth_identity_budget_status(status: &GitHubRateLimitStatus) -> AppResult<()> {
        let requested_cost = 1;
        if status.remaining.saturating_sub(requested_cost) < GITHUB_CORE_REST_MINIMUM_RESERVE {
            return Err(AppError::RateBudgetReserved {
                operation: "github_oauth_identity".to_string(),
                remaining: status.remaining,
                reserve: GITHUB_CORE_REST_MINIMUM_RESERVE,
                requested_cost,
                reset_at: status.reset_at,
            });
        }
        Ok(())
    }

    async fn poll_device_access_token(
        &self,
        codes: &StoredDeviceCodes,
        config: &GitHubAuthConfig,
    ) -> AppResult<String> {
        let client = Self::oauth_client()?;
        let mut interval = codes.interval.max(1);

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            let response: serde_json::Value = client
                .post("https://github.com/login/oauth/access_token")
                .header("Accept", "application/json")
                .json(&DeviceAccessTokenRequest {
                    client_id: config.client_id.expose_secret().to_string(),
                    device_code: codes.device_code.clone(),
                    grant_type: "urn:ietf:params:oauth:grant-type:device_code".to_string(),
                })
                .send()
                .await
                .map_err(|error| AppError::External(error.to_string()))?
                .json()
                .await
                .map_err(|error| AppError::External(error.to_string()))?;

            if let Some(access_token) = response
                .get("access_token")
                .and_then(|value| value.as_str())
            {
                return Ok(access_token.to_string());
            }

            match response.get("error").and_then(|value| value.as_str()) {
                Some("authorization_pending") => continue,
                Some("slow_down") => interval += 5,
                Some(error) => {
                    return Err(AppError::External(format!(
                        "device flow failed with GitHub error `{error}`"
                    )));
                }
                None => {
                    return Err(AppError::External(
                        "device flow returned an unexpected response".to_string(),
                    ));
                }
            }
        }
    }

    fn parse_device_codes(device_code: &str) -> AppResult<StoredDeviceCodes> {
        serde_json::from_str(device_code)
            .map_err(|error| AppError::Validation(format!("invalid device code payload: {error}")))
    }

    async fn bounded_pages<T: DeserializeOwned>(
        crab: &octocrab::Octocrab,
        mut page: Page<T>,
        limit: usize,
    ) -> AppResult<BoundedCollection<T>> {
        let mut items = page.take_items();
        let truncated_within_page = items.len() > limit;
        items.truncate(limit);

        while items.len() < limit {
            let Some(mut next_page) = crab
                .get_page(&page.next)
                .await
                .map_err(|error| AppError::External(error.to_string()))?
            else {
                break;
            };
            let mut next_items = next_page.take_items();
            next_items.truncate(limit - items.len());
            items.append(&mut next_items);
            page = next_page;
        }

        Ok(BoundedCollection {
            complete: !truncated_within_page && page.next.is_none(),
            items,
        })
    }

    fn repository_is_public(repository: &octocrab::models::Repository) -> bool {
        !repository.private.unwrap_or(false)
            && repository
                .visibility
                .as_deref()
                .is_none_or(|visibility| visibility.eq_ignore_ascii_case("public"))
    }
}

#[async_trait]
impl GitHubClientPort for OctocrabGitHubClient {
    async fn start_device_flow(&self, config: &GitHubAuthConfig) -> AppResult<DeviceLoginStart> {
        let codes = self
            .auth_client
            .authenticate_as_device(&config.client_id, config.scopes.clone())
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        Ok(DeviceLoginStart {
            verification_uri: codes.verification_uri.clone(),
            user_code: codes.user_code.clone(),
            device_code: serde_json::to_string(&StoredDeviceCodes::from(codes))
                .map_err(|error| AppError::External(error.to_string()))?,
        })
    }

    async fn finish_device_flow(
        &self,
        config: &GitHubAuthConfig,
        device_code: &str,
    ) -> AppResult<GitHubConnection> {
        let codes = Self::parse_device_codes(device_code)?;
        let token = tokio::time::timeout(
            StdDuration::from_secs(codes.expires_in.saturating_add(5)),
            self.poll_device_access_token(&codes, config),
        )
        .await
        .map_err(|_| AppError::External("GitHub device authorization expired".to_string()))??;
        let user_client = Self::user_client(&token)?;
        Self::ensure_oauth_identity_budget(&user_client).await?;
        let viewer: GitHubApiUser = user_client
            .get("/user", None::<&()>)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        Ok(GitHubConnection {
            account: ConnectedAccount {
                github_user_id: viewer.id,
                login: viewer.login,
                display_name: viewer.name,
            },
            access_token: token,
            scopes: config.scopes.clone(),
        })
    }

    async fn exchange_browser_code(
        &self,
        config: &GitHubAuthConfig,
        code: &str,
    ) -> AppResult<GitHubConnection> {
        let client_secret = config.client_secret.as_ref().ok_or_else(|| {
            AppError::Config("browser OAuth requires a client secret".to_string())
        })?;
        let request = BrowserCodeExchangeRequest {
            client_id: config.client_id.expose_secret().to_string(),
            client_secret: client_secret.expose_secret().to_string(),
            code: code.to_string(),
            redirect_uri: config.redirect_uri.clone(),
        };
        let oauth: BrowserOAuthResponse = Self::oauth_client()?
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .json()
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let token = oauth.access_token;
        let user_client = Self::user_client(&token)?;
        Self::ensure_oauth_identity_budget(&user_client).await?;
        let viewer: GitHubApiUser = user_client
            .get("/user", None::<&()>)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        Ok(GitHubConnection {
            account: ConnectedAccount {
                github_user_id: viewer.id,
                login: viewer.login,
                display_name: viewer.name,
            },
            access_token: token,
            scopes: oauth.scope.split(',').map(ToString::to_string).collect(),
        })
    }

    async fn fetch_graph(&self, connection: &GitHubConnection) -> AppResult<GraphImport> {
        self.fetch_user_graph(connection, &connection.account.login)
            .await
    }

    async fn fetch_user_graph(
        &self,
        connection: &GitHubConnection,
        login: &str,
    ) -> AppResult<GraphImport> {
        let crab = Self::user_client(&connection.access_token)?;
        let viewer: GitHubApiUser = crab
            .get(format!("/users/{login}"), None::<&()>)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let followers: Page<GitHubApiUser> = crab
            .get(
                format!("/users/{login}/followers"),
                Some(&GitHubListParams::default()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let followers =
            Self::bounded_pages(&crab, followers, MAX_SOCIAL_CONNECTIONS_PER_EXPANSION).await?;

        let following: Page<GitHubApiUser> = crab
            .get(
                format!("/users/{login}/following"),
                Some(&GitHubListParams::default()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let following =
            Self::bounded_pages(&crab, following, MAX_SOCIAL_CONNECTIONS_PER_EXPANSION).await?;

        let starred: Page<octocrab::models::Repository> = crab
            .get(
                format!("/users/{login}/starred"),
                Some(&GitHubListParams::default()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let starred = Self::bounded_pages(&crab, starred, MAX_REPOSITORIES_PER_EXPANSION).await?;

        let repositories: Page<octocrab::models::Repository> = crab
            .get(
                format!("/users/{login}/repos"),
                Some(&GitHubRepositoryListParams::default()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let repositories =
            Self::bounded_pages(&crab, repositories, MAX_REPOSITORIES_PER_EXPANSION).await?;

        Ok(GraphImport {
            viewer: Some(viewer.into()),
            followers: followers.items.into_iter().map(Into::into).collect(),
            following: following.items.into_iter().map(Into::into).collect(),
            starred_repositories: starred
                .items
                .into_iter()
                .filter(Self::repository_is_public)
                .map(Into::into)
                .collect(),
            repositories: repositories
                .items
                .into_iter()
                .filter(Self::repository_is_public)
                .map(Into::into)
                .collect(),
            coverage: GraphImportCoverage {
                followers_complete: followers.complete,
                following_complete: following.complete,
                starred_repositories_complete: starred.complete,
                repositories_complete: repositories.complete,
            },
        })
    }

    async fn fetch_core_rate_limit(
        &self,
        connection: &GitHubConnection,
    ) -> AppResult<GitHubRateLimitStatus> {
        let crab = Self::user_client(&connection.access_token)?;
        Self::fetch_core_rate_limit_for_client(&crab).await
    }

    async fn fetch_repository_contributors(
        &self,
        connection: &GitHubConnection,
        full_name: &str,
    ) -> AppResult<RepositoryContributorsSnapshot> {
        let crab = Self::user_client(&connection.access_token)?;
        let page: Page<GitHubApiContributor> = crab
            .get(
                format!("/repos/{full_name}/contributors"),
                Some(&GitHubContributorParams::default()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let contributors = Self::bounded_pages(&crab, page, REPOSITORY_CONTRIBUTOR_LIMIT).await?;
        Ok(RepositoryContributorsSnapshot {
            contributors: contributors
                .items
                .into_iter()
                .map(|contributor| RepositoryContributor {
                    github_id: contributor.id,
                    login: contributor.login,
                    avatar_url: contributor.avatar_url,
                    url: contributor.html_url,
                    contributions: contributor.contributions,
                })
                .collect(),
            source_complete: contributors.complete,
        })
    }

    async fn fetch_user_commit_repositories(
        &self,
        connection: &GitHubConnection,
        login: &str,
    ) -> AppResult<UserCommitRepositoriesSnapshot> {
        let crab = Self::user_client(&connection.access_token)?;
        let page: Page<GitHubPublicEvent> = crab
            .get(
                format!("/users/{login}/events/public"),
                Some(&GitHubListParams::default()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let events = Self::bounded_pages(&crab, page, USER_COMMIT_ACTIVITY_EVENT_LIMIT).await?;
        Ok(aggregate_user_commit_events(events.items, events.complete))
    }

    fn browser_oauth_url(&self, config: &GitHubAuthConfig, state: &str) -> AppResult<String> {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        serializer.append_pair("client_id", config.client_id.expose_secret());
        serializer.append_pair("state", state);
        if let Some(redirect_uri) = &config.redirect_uri {
            serializer.append_pair("redirect_uri", redirect_uri);
        }
        if !config.scopes.is_empty() {
            serializer.append_pair("scope", &config.scopes.join(" "));
        }
        Ok(format!(
            "https://github.com/login/oauth/authorize?{}",
            serializer.finish()
        ))
    }
}

#[derive(Default)]
pub struct StubGitHubClient {
    pub import: GraphImport,
}

#[async_trait]
impl GitHubClientPort for StubGitHubClient {
    async fn start_device_flow(&self, _config: &GitHubAuthConfig) -> AppResult<DeviceLoginStart> {
        Ok(DeviceLoginStart {
            verification_uri: "https://example.test/device".to_string(),
            user_code: "ABCD-EFGH".to_string(),
            device_code: "stub-device-code".to_string(),
        })
    }

    async fn finish_device_flow(
        &self,
        _config: &GitHubAuthConfig,
        _device_code: &str,
    ) -> AppResult<GitHubConnection> {
        Ok(GitHubConnection {
            account: ConnectedAccount {
                github_user_id: 1,
                login: "stub-user".to_string(),
                display_name: Some("Stub User".to_string()),
            },
            access_token: "stub-token".to_string(),
            scopes: vec!["read:user".to_string()],
        })
    }

    async fn exchange_browser_code(
        &self,
        _config: &GitHubAuthConfig,
        _code: &str,
    ) -> AppResult<GitHubConnection> {
        self.finish_device_flow(
            &GitHubAuthConfig {
                client_id: SecretString::from("stub"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec![],
            },
            "stub",
        )
        .await
    }

    async fn fetch_graph(&self, _connection: &GitHubConnection) -> AppResult<GraphImport> {
        Ok(self.import.clone())
    }

    async fn fetch_user_graph(
        &self,
        _connection: &GitHubConnection,
        _login: &str,
    ) -> AppResult<GraphImport> {
        Ok(self.import.clone())
    }

    async fn fetch_core_rate_limit(
        &self,
        _connection: &GitHubConnection,
    ) -> AppResult<GitHubRateLimitStatus> {
        Ok(GitHubRateLimitStatus {
            limit: 5_000,
            used: 0,
            remaining: 5_000,
            reset_at: Utc::now() + Duration::hours(1),
            checked_at: Utc::now(),
        })
    }

    async fn fetch_repository_contributors(
        &self,
        _connection: &GitHubConnection,
        _full_name: &str,
    ) -> AppResult<RepositoryContributorsSnapshot> {
        Ok(RepositoryContributorsSnapshot {
            contributors: Vec::new(),
            source_complete: true,
        })
    }

    async fn fetch_user_commit_repositories(
        &self,
        _connection: &GitHubConnection,
        _login: &str,
    ) -> AppResult<UserCommitRepositoriesSnapshot> {
        Ok(UserCommitRepositoriesSnapshot {
            repositories: Vec::new(),
            source_event_count: 0,
            source_truncated: false,
        })
    }

    fn browser_oauth_url(&self, _config: &GitHubAuthConfig, state: &str) -> AppResult<String> {
        Ok(format!("https://example.test/oauth?state={state}"))
    }
}

pub struct Neo4jClient {
    pub(crate) graph: Graph,
    pub(crate) database: String,
}

impl Neo4jClient {
    pub async fn new(config: &Neo4jConfig) -> AppResult<Self> {
        let graph = Graph::new(
            config
                .uri
                .clone()
                .ok_or_else(|| AppError::Config("neo4j uri missing".to_string()))?,
            config
                .username
                .clone()
                .ok_or_else(|| AppError::Config("neo4j username missing".to_string()))?,
            config
                .password
                .as_ref()
                .ok_or_else(|| AppError::Config("neo4j password missing".to_string()))?
                .expose_secret()
                .to_string(),
        )
        .await
        .map_err(|error| AppError::External(error.to_string()))?;
        Ok(Self {
            graph,
            database: config.database.clone(),
        })
    }

    pub(crate) async fn run(&self, q: neo4rs::Query) -> AppResult<()> {
        self.graph
            .run_on(&self.database, q)
            .await
            .map_err(|error| AppError::External(error.to_string()))
    }
}

async fn run_in_transaction(transaction: &mut Txn, query: neo4rs::Query) -> AppResult<()> {
    transaction
        .run(query)
        .await
        .map_err(|error| AppError::External(error.to_string()))
}

struct Neo4jGitHubImportRepository {
    client: Arc<Neo4jClient>,
    max_total_nodes: Option<usize>,
    max_total_relationships: Option<usize>,
}

struct Neo4jIdentityRepository {
    client: Arc<Neo4jClient>,
    cipher: Arc<IdentityCipher>,
}

struct Neo4jSyncStateRepository {
    client: Arc<Neo4jClient>,
}

struct Neo4jCategoryRepository {
    client: Arc<Neo4jClient>,
}

struct Neo4jBookmarkRepository {
    client: Arc<Neo4jClient>,
}

struct Neo4jExplorationRepository {
    client: Arc<Neo4jClient>,
}

struct Neo4jDiscoveryRepository {
    client: Arc<Neo4jClient>,
}

struct Neo4jInsightRepository {
    client: Arc<Neo4jClient>,
}

impl Neo4jIdentityRepository {
    async fn cleanup_expired(&self) -> AppResult<()> {
        self.client
            .run(query(
                "MATCH (pending:OAuthPendingState)
                 WHERE pending.expires_at <= datetime()
                 DETACH DELETE pending",
            ))
            .await?;
        self.client
            .run(query(
                "MATCH (session:BrowserSession)
                 WHERE session.expires_at <= datetime()
                 DETACH DELETE session",
            ))
            .await
    }

    async fn import_account_link(&self, user_id: &str, github_user_id: i64) -> AppResult<()> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MERGE (identity:GitHubIdentity {github_user_id: $github_user_id})
                     ON CREATE SET identity.user_id = $user_id,
                                   identity.created_at = datetime()
                     WITH identity
                     WHERE identity.user_id = $user_id
                     MERGE (local:LocalUser {id: identity.user_id})
                     MERGE (local)-[:HAS_GITHUB_IDENTITY]->(identity)
                     RETURN identity.user_id AS user_id",
                )
                .param("github_user_id", github_user_id)
                .param("user_id", user_id.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        if rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .is_none()
        {
            return Err(AppError::Validation(format!(
                "GitHub account {github_user_id} is already linked to another GitExplore identity"
            )));
        }
        Ok(())
    }

    async fn import_session(
        &self,
        session_id: &str,
        user_id: &str,
        expires_at: DateTime<Utc>,
    ) -> AppResult<()> {
        if expires_at <= Utc::now() {
            return Ok(());
        }
        let session_digest = self.cipher.digest("browser-session", session_id)?;
        self.client
            .run(
                query(
                    "MERGE (local:LocalUser {id: $user_id})
                     MERGE (session:BrowserSession {id_digest: $session_digest})
                     SET session.user_id = $user_id,
                         session.expires_at = datetime($expires_at),
                         session.updated_at = datetime()
                     MERGE (local)-[:HAS_SESSION]->(session)",
                )
                .param("user_id", user_id.to_string())
                .param("session_digest", session_digest)
                .param("expires_at", expires_at.to_rfc3339()),
            )
            .await
    }
}

#[async_trait]
impl IdentityRepository for Neo4jIdentityRepository {
    async fn get_connection(&self, user_id: &str) -> AppResult<Option<GitHubConnection>> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (identity:GitHubIdentity {user_id: $user_id})
                     WHERE identity.access_token_ciphertext IS NOT NULL
                     RETURN identity.github_user_id AS github_user_id,
                            identity.login AS login,
                            identity.display_name AS display_name,
                            identity.scopes AS scopes,
                            identity.access_token_ciphertext AS access_token_ciphertext",
                )
                .param("user_id", user_id.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(None);
        };
        let github_user_id = row.get::<i64>("github_user_id").map_err(map_neo4j_decode)?;
        let ciphertext = row
            .get::<String>("access_token_ciphertext")
            .map_err(map_neo4j_decode)?;
        let access_token = self.cipher.decrypt(
            GITHUB_TOKEN_PURPOSE,
            &github_user_id.to_string(),
            &ciphertext,
        )?;
        Ok(Some(GitHubConnection {
            account: ConnectedAccount {
                github_user_id,
                login: row.get::<String>("login").map_err(map_neo4j_decode)?,
                display_name: row
                    .get::<Option<String>>("display_name")
                    .map_err(map_neo4j_decode)?,
            },
            access_token,
            scopes: row
                .get::<Option<Vec<String>>>("scopes")
                .map_err(map_neo4j_decode)?
                .unwrap_or_default(),
        }))
    }

    async fn save_connection(&self, user_id: &str, connection: GitHubConnection) -> AppResult<()> {
        let github_user_id = connection.account.github_user_id;
        let ciphertext = self.cipher.encrypt(
            GITHUB_TOKEN_PURPOSE,
            &github_user_id.to_string(),
            &connection.access_token,
        )?;
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MERGE (identity:GitHubIdentity {github_user_id: $github_user_id})
                     ON CREATE SET identity.user_id = $user_id,
                                   identity.created_at = datetime()
                     WITH identity
                     WHERE identity.user_id = $user_id
                     MERGE (local:LocalUser {id: identity.user_id})
                     MERGE (local)-[:HAS_GITHUB_IDENTITY]->(identity)
                     SET identity.login = $login,
                         identity.display_name = CASE WHEN $display_name = '' THEN null ELSE $display_name END,
                         identity.scopes = $scopes,
                         identity.access_token_ciphertext = $ciphertext,
                         identity.connected_at = datetime()
                     RETURN identity.user_id AS user_id",
                )
                .param("github_user_id", github_user_id)
                .param("user_id", user_id.to_string())
                .param("login", connection.account.login)
                .param(
                    "display_name",
                    connection.account.display_name.unwrap_or_default(),
                )
                .param("scopes", connection.scopes)
                .param("ciphertext", ciphertext),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        if rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .is_none()
        {
            return Err(AppError::Validation(
                "this GitHub account is already linked to another GitExplore identity".to_string(),
            ));
        }
        Ok(())
    }

    async fn save_browser_connection(
        &self,
        preferred_user_id: &str,
        connection: GitHubConnection,
    ) -> AppResult<String> {
        let github_user_id = connection.account.github_user_id;
        let ciphertext = self.cipher.encrypt(
            GITHUB_TOKEN_PURPOSE,
            &github_user_id.to_string(),
            &connection.access_token,
        )?;
        let fallback_user_id = Uuid::new_v4().to_string();
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "OPTIONAL MATCH (existing:GitHubIdentity {github_user_id: $github_user_id})
                     WITH existing
                     OPTIONAL MATCH (preferred:GitHubIdentity {user_id: $preferred_user_id})
                     WITH existing, preferred,
                          CASE
                            WHEN existing IS NOT NULL THEN existing.user_id
                            WHEN preferred IS NULL THEN $preferred_user_id
                            ELSE $fallback_user_id
                          END AS candidate_user_id
                     MERGE (identity:GitHubIdentity {github_user_id: $github_user_id})
                     ON CREATE SET identity.user_id = candidate_user_id,
                                   identity.created_at = datetime()
                     WITH identity
                     MERGE (local:LocalUser {id: identity.user_id})
                     MERGE (local)-[:HAS_GITHUB_IDENTITY]->(identity)
                     SET identity.login = $login,
                         identity.display_name = CASE WHEN $display_name = '' THEN null ELSE $display_name END,
                         identity.scopes = $scopes,
                         identity.access_token_ciphertext = $ciphertext,
                         identity.connected_at = datetime()
                     RETURN identity.user_id AS user_id",
                )
                .param("github_user_id", github_user_id)
                .param("preferred_user_id", preferred_user_id.to_string())
                .param("fallback_user_id", fallback_user_id)
                .param("login", connection.account.login)
                .param(
                    "display_name",
                    connection.account.display_name.unwrap_or_default(),
                )
                .param("scopes", connection.scopes)
                .param("ciphertext", ciphertext),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let row = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .ok_or_else(|| {
                AppError::Storage("canonical GitHub identity was not returned".to_string())
            })?;
        row.get::<String>("user_id").map_err(map_neo4j_decode)
    }

    async fn clear_connection(&self, user_id: &str) -> AppResult<()> {
        self.client
            .run(
                query(
                    "MATCH (identity:GitHubIdentity {user_id: $user_id})
                     REMOVE identity.login,
                            identity.display_name,
                            identity.scopes,
                            identity.access_token_ciphertext,
                            identity.connected_at",
                )
                .param("user_id", user_id.to_string()),
            )
            .await
    }

    async fn save_pending_browser_login(
        &self,
        state_id: &str,
        pending: PendingBrowserLogin,
    ) -> AppResult<()> {
        self.cleanup_expired().await?;
        let state_digest = self.cipher.digest("oauth-state", state_id)?;
        let encrypted_nonce =
            self.cipher
                .encrypt(BROWSER_NONCE_PURPOSE, state_id, &pending.browser_nonce)?;
        self.client
            .run(
                query(
                    "MERGE (pending:OAuthPendingState {state_digest: $state_digest})
                     SET pending.user_id = $user_id,
                         pending.redirect_to = CASE WHEN $redirect_to = '' THEN null ELSE $redirect_to END,
                         pending.browser_nonce_ciphertext = $browser_nonce_ciphertext,
                         pending.created_at = datetime($created_at),
                         pending.expires_at = datetime($expires_at)",
                )
                .param("state_digest", state_digest)
                .param("user_id", pending.user_id)
                .param("redirect_to", pending.redirect_to.unwrap_or_default())
                .param("browser_nonce_ciphertext", encrypted_nonce)
                .param("created_at", pending.created_at.to_rfc3339())
                .param("expires_at", pending.expires_at.to_rfc3339()),
            )
            .await?;
        self.client
            .run(
                query(
                    "MATCH (pending:OAuthPendingState)
                     WITH pending
                     ORDER BY pending.created_at DESC
                     SKIP $limit
                     DETACH DELETE pending",
                )
                .param("limit", MAX_PENDING_BROWSER_LOGINS as i64),
            )
            .await
    }

    async fn consume_pending_browser_login(
        &self,
        state_id: &str,
    ) -> AppResult<Option<PendingBrowserLogin>> {
        let state_digest = self.cipher.digest("oauth-state", state_id)?;
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (pending:OAuthPendingState {state_digest: $state_digest})
                     WITH pending,
                          pending.user_id AS user_id,
                          pending.redirect_to AS redirect_to,
                          pending.browser_nonce_ciphertext AS browser_nonce_ciphertext,
                          toString(pending.created_at) AS created_at,
                          toString(pending.expires_at) AS expires_at
                     DELETE pending
                     RETURN user_id, redirect_to, browser_nonce_ciphertext, created_at, expires_at",
                )
                .param("state_digest", state_digest),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(None);
        };
        let nonce_ciphertext = row
            .get::<String>("browser_nonce_ciphertext")
            .map_err(map_neo4j_decode)?;
        Ok(Some(PendingBrowserLogin {
            user_id: row.get::<String>("user_id").map_err(map_neo4j_decode)?,
            redirect_to: row
                .get::<Option<String>>("redirect_to")
                .map_err(map_neo4j_decode)?,
            browser_nonce: self.cipher.decrypt(
                BROWSER_NONCE_PURPOSE,
                state_id,
                &nonce_ciphertext,
            )?,
            created_at: parse_required_timestamp(
                &row.get::<String>("created_at").map_err(map_neo4j_decode)?,
            )?,
            expires_at: parse_required_timestamp(
                &row.get::<String>("expires_at").map_err(map_neo4j_decode)?,
            )?,
        }))
    }

    async fn create_session(&self, session_id: &str, user_id: &str) -> AppResult<()> {
        self.cleanup_expired().await?;
        self.import_session(
            session_id,
            user_id,
            Utc::now() + Duration::days(SESSION_TTL_DAYS),
        )
        .await?;
        self.client
            .run(
                query(
                    "MATCH (session:BrowserSession)
                     WITH session
                     ORDER BY session.expires_at DESC
                     SKIP $limit
                     DETACH DELETE session",
                )
                .param("limit", MAX_ACTIVE_SESSIONS as i64),
            )
            .await
    }

    async fn get_user_id_for_session(&self, session_id: &str) -> AppResult<Option<String>> {
        self.cleanup_expired().await?;
        let session_digest = self.cipher.digest("browser-session", session_id)?;
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (session:BrowserSession {id_digest: $session_digest})
                     WHERE session.expires_at > datetime()
                     RETURN session.user_id AS user_id",
                )
                .param("session_digest", session_digest),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        rows.next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .map(|row| row.get::<String>("user_id").map_err(map_neo4j_decode))
            .transpose()
    }

    async fn clear_session(&self, session_id: &str) -> AppResult<()> {
        let session_digest = self.cipher.digest("browser-session", session_id)?;
        self.client
            .run(
                query(
                    "MATCH (session:BrowserSession {id_digest: $session_digest})
                     DETACH DELETE session",
                )
                .param("session_digest", session_digest),
            )
            .await
    }

    async fn github_rate_limit(
        &self,
        github_user_id: i64,
    ) -> AppResult<Option<GitHubRateLimitStatus>> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (identity:GitHubIdentity {github_user_id: $github_user_id})
                     WHERE identity.core_rate_limit_remaining IS NOT NULL
                       AND identity.core_rate_limit_reset_at IS NOT NULL
                       AND identity.core_rate_limit_observed_at IS NOT NULL
                     RETURN coalesce(identity.core_rate_limit_limit, 0) AS rate_limit,
                            coalesce(identity.core_rate_limit_used, 0) AS used,
                            identity.core_rate_limit_remaining AS remaining,
                            toString(identity.core_rate_limit_reset_at) AS reset_at,
                            toString(identity.core_rate_limit_observed_at) AS observed_at",
                )
                .param("github_user_id", github_user_id),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(None);
        };
        Ok(Some(GitHubRateLimitStatus {
            limit: non_negative_i64_to_usize(
                row.get::<i64>("rate_limit").map_err(map_neo4j_decode)?,
                "core rate limit",
            )?,
            used: non_negative_i64_to_usize(
                row.get::<i64>("used").map_err(map_neo4j_decode)?,
                "core rate-limit used count",
            )?,
            remaining: non_negative_i64_to_usize(
                row.get::<i64>("remaining").map_err(map_neo4j_decode)?,
                "core rate-limit remaining count",
            )?,
            reset_at: parse_required_timestamp(
                &row.get::<String>("reset_at").map_err(map_neo4j_decode)?,
            )?,
            checked_at: parse_required_timestamp(
                &row.get::<String>("observed_at").map_err(map_neo4j_decode)?,
            )?,
        }))
    }

    async fn save_github_rate_limit(
        &self,
        github_user_id: i64,
        status: GitHubRateLimitStatus,
    ) -> AppResult<()> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (identity:GitHubIdentity {github_user_id: $github_user_id})
                     SET identity.core_rate_limit_limit = $rate_limit,
                         identity.core_rate_limit_used = $used,
                         identity.core_rate_limit_remaining = $remaining,
                         identity.core_rate_limit_reset_at = datetime($reset_at),
                         identity.core_rate_limit_observed_at = datetime($observed_at)
                     RETURN count(identity) AS updated",
                )
                .param("github_user_id", github_user_id)
                .param("rate_limit", usize_to_i64(status.limit, "core rate limit")?)
                .param(
                    "used",
                    usize_to_i64(status.used, "core rate-limit used count")?,
                )
                .param(
                    "remaining",
                    usize_to_i64(status.remaining, "core rate-limit remaining count")?,
                )
                .param("reset_at", status.reset_at.to_rfc3339())
                .param("observed_at", status.checked_at.to_rfc3339()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let updated = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .and_then(|row| row.get::<i64>("updated").ok())
            .unwrap_or_default();
        if updated == 1 {
            Ok(())
        } else {
            Err(AppError::Storage(format!(
                "GitHub identity {github_user_id} is missing while saving its rate budget"
            )))
        }
    }

    async fn try_acquire_github_rate_limit_lease(
        &self,
        github_user_id: i64,
        token: &str,
        lease_seconds: i64,
    ) -> AppResult<Option<GitHubRateLimitLease>> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (identity:GitHubIdentity {github_user_id: $github_user_id})
                     SET identity.core_rate_limit_mutex = coalesce(identity.core_rate_limit_mutex, 0) + 1
                     WITH identity,
                          identity.core_rate_limit_lease_token IS NULL
                            OR identity.core_rate_limit_lease_expires_at IS NULL
                            OR identity.core_rate_limit_lease_expires_at <= datetime() AS available
                     FOREACH (_ IN CASE WHEN available THEN [1] ELSE [] END |
                       SET identity.core_rate_limit_lease_token = $token,
                           identity.core_rate_limit_lease_expires_at = datetime() + duration({seconds: $lease_seconds})
                     )
                     RETURN available AS acquired,
                            toString(identity.core_rate_limit_lease_expires_at) AS expires_at",
                )
                .param("github_user_id", github_user_id)
                .param("token", token.to_string())
                .param("lease_seconds", lease_seconds),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let row = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .ok_or_else(|| {
                AppError::Storage(format!(
                    "GitHub identity {github_user_id} is missing while acquiring its rate budget"
                ))
            })?;
        if !row.get::<bool>("acquired").map_err(map_neo4j_decode)? {
            return Ok(None);
        }
        let expires_at = optional_timestamp(&row, "expires_at")?.ok_or_else(|| {
            AppError::Storage("acquired GitHub rate-limit lease has no expiry".to_string())
        })?;
        Ok(Some(GitHubRateLimitLease {
            github_user_id,
            token: token.to_string(),
            expires_at,
        }))
    }

    async fn renew_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
        lease_seconds: i64,
    ) -> AppResult<bool> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (identity:GitHubIdentity {github_user_id: $github_user_id})
                     SET identity.core_rate_limit_mutex = coalesce(identity.core_rate_limit_mutex, 0) + 1
                     WITH identity,
                          identity.core_rate_limit_lease_token = $token
                            AND identity.core_rate_limit_lease_expires_at > datetime() AS valid
                     FOREACH (_ IN CASE WHEN valid THEN [1] ELSE [] END |
                       SET identity.core_rate_limit_lease_expires_at = datetime() + duration({seconds: $lease_seconds})
                     )
                     RETURN valid AS renewed",
                )
                .param("github_user_id", lease.github_user_id)
                .param("token", lease.token.clone())
                .param("lease_seconds", lease_seconds),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        Ok(rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .and_then(|row| row.get::<bool>("renewed").ok())
            .unwrap_or(false))
    }

    async fn release_github_rate_limit_lease(
        &self,
        lease: &GitHubRateLimitLease,
    ) -> AppResult<bool> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (identity:GitHubIdentity {github_user_id: $github_user_id})
                     SET identity.core_rate_limit_mutex = coalesce(identity.core_rate_limit_mutex, 0) + 1
                     WITH identity, identity.core_rate_limit_lease_token = $token AS valid
                     FOREACH (_ IN CASE WHEN valid THEN [1] ELSE [] END |
                       REMOVE identity.core_rate_limit_lease_token,
                              identity.core_rate_limit_lease_expires_at
                     )
                     RETURN valid AS released",
                )
                .param("github_user_id", lease.github_user_id)
                .param("token", lease.token.clone()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        Ok(rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .and_then(|row| row.get::<bool>("released").ok())
            .unwrap_or(false))
    }
}

const GRAPH_IMPORT_CAPACITY_GATE_ENTITY_KEY: &str = "__gitexplore:graph-import-capacity";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GraphImportCapacityEstimate {
    nodes: usize,
    relationships: usize,
}

impl GraphImportCapacityEstimate {
    fn from_import(import: &GraphImport) -> Self {
        let mut user_ids = HashSet::new();
        let mut repository_ids = HashSet::new();
        let mut follows = HashSet::new();
        let mut owned = HashSet::new();
        let mut starred = HashSet::new();

        if let Some(viewer) = &import.viewer {
            user_ids.insert(viewer.github_id);
            for follower in &import.followers {
                user_ids.insert(follower.github_id);
                follows.insert((follower.github_id, viewer.github_id));
            }
            for following in &import.following {
                user_ids.insert(following.github_id);
                follows.insert((viewer.github_id, following.github_id));
            }
            for repository in &import.repositories {
                repository_ids.insert(repository.github_id);
                owned.insert(repository.github_id);
            }
            for repository in &import.starred_repositories {
                repository_ids.insert(repository.github_id);
                starred.insert(repository.github_id);
            }
        } else {
            user_ids.extend(import.followers.iter().map(|user| user.github_id));
            user_ids.extend(import.following.iter().map(|user| user.github_id));
            repository_ids.extend(
                import
                    .repositories
                    .iter()
                    .chain(import.starred_repositories.iter())
                    .map(|repository| repository.github_id),
            );
        }

        Self {
            nodes: user_ids.len().saturating_add(repository_ids.len()),
            relationships: follows
                .len()
                .saturating_add(owned.len())
                .saturating_add(starred.len()),
        }
    }
}

fn enforce_graph_capacity(
    current_nodes: usize,
    current_relationships: usize,
    incoming: GraphImportCapacityEstimate,
    max_total_nodes: Option<usize>,
    max_total_relationships: Option<usize>,
) -> AppResult<()> {
    let projected_nodes = current_nodes.saturating_add(incoming.nodes);
    if let Some(maximum_count) = max_total_nodes
        && projected_nodes > maximum_count
    {
        return Err(AppError::GraphCapacityExceeded {
            resource: "nodes".to_string(),
            current_count: current_nodes,
            incoming_count: incoming.nodes,
            projected_count: projected_nodes,
            maximum_count,
        });
    }

    let projected_relationships = current_relationships.saturating_add(incoming.relationships);
    if let Some(maximum_count) = max_total_relationships
        && projected_relationships > maximum_count
    {
        return Err(AppError::GraphCapacityExceeded {
            resource: "relationships".to_string(),
            current_count: current_relationships,
            incoming_count: incoming.relationships,
            projected_count: projected_relationships,
            maximum_count,
        });
    }
    Ok(())
}

impl Neo4jGitHubImportRepository {
    async fn enforce_capacity_before_import(
        &self,
        transaction: &mut Txn,
        incoming: GraphImportCapacityEstimate,
    ) -> AppResult<()> {
        if self.max_total_nodes.is_none() && self.max_total_relationships.is_none() {
            return Ok(());
        }

        let mut rows = transaction
            .execute(
                query(
                    "MERGE (capacity:RefreshLease {entity_key: $entity_key})
                     ON CREATE SET capacity.status = 'capacity_guard', capacity.mutex = 0
                     SET capacity.mutex = coalesce(capacity.mutex, 0) + 1,
                         capacity.last_checked_at = datetime()
                     WITH capacity
                     OPTIONAL MATCH (node)
                     WITH capacity, count(node) AS current_nodes
                     OPTIONAL MATCH ()-[relationship]->()
                     RETURN current_nodes, count(relationship) AS current_relationships",
                )
                .param(
                    "entity_key",
                    GRAPH_IMPORT_CAPACITY_GATE_ENTITY_KEY.to_string(),
                ),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let row = rows
            .next(transaction)
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .ok_or_else(|| AppError::Storage("Neo4j capacity query returned no row".to_string()))?;
        let current_nodes = non_negative_i64_to_usize(
            row.get::<i64>("current_nodes").map_err(map_neo4j_decode)?,
            "Neo4j total node count",
        )?;
        let current_relationships = non_negative_i64_to_usize(
            row.get::<i64>("current_relationships")
                .map_err(map_neo4j_decode)?,
            "Neo4j total relationship count",
        )?;
        enforce_graph_capacity(
            current_nodes,
            current_relationships,
            incoming,
            self.max_total_nodes,
            self.max_total_relationships,
        )
    }

    async fn import_github_graph_inner(
        &self,
        _user_id: &str,
        import: GraphImport,
        refresh: Option<(&RefreshLease, &str)>,
    ) -> AppResult<SyncSummary> {
        let viewer = import.viewer.clone().ok_or_else(|| {
            AppError::Validation("viewer missing from GitHub graph import".to_string())
        })?;
        let capacity_estimate = GraphImportCapacityEstimate::from_import(&import);
        let coverage = import.coverage;
        let fetched_at = Utc::now().to_rfc3339();
        let stale_at = if coverage.is_complete() {
            Utc::now() + Duration::hours(6)
        } else {
            Utc::now()
        }
        .to_rfc3339();
        let mut transaction = self
            .client
            .graph
            .start_txn_on(self.client.database.clone())
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        if let Err(error) = self
            .enforce_capacity_before_import(&mut transaction, capacity_estimate)
            .await
        {
            if let Err(rollback_error) = transaction.rollback().await {
                tracing::warn!(
                    %rollback_error,
                    "failed to explicitly roll back a rejected Neo4j graph import"
                );
            }
            return Err(error);
        }
        if let Some((lease, _)) = refresh {
            let mut rows = transaction
                .execute(
                    query(
                        "MATCH (lease:RefreshLease {entity_key: $entity_key})
                         SET lease.mutex = coalesce(lease.mutex, 0) + 1
                         WITH lease
                         WHERE lease.status = 'running'
                           AND lease.token = $token
                           AND lease.expires_at > datetime()
                         RETURN count(lease) AS valid",
                    )
                    .param("entity_key", lease.entity_key.clone())
                    .param("token", lease.token.clone()),
                )
                .await
                .map_err(|error| AppError::External(error.to_string()))?;
            let valid = rows
                .next(&mut transaction)
                .await
                .map_err(|error| AppError::External(error.to_string()))?
                .and_then(|row| row.get::<i64>("valid").ok())
                .unwrap_or_default();
            if valid != 1 {
                return Err(AppError::External(
                    "refresh lease was lost before graph import".to_string(),
                ));
            }
        }
        run_in_transaction(
            &mut transaction,
                query(
                    "OPTIONAL MATCH (alias:User {login_key: $login_key})
                     WHERE alias.github_id <> $github_id
                     SET alias.login = '__gitexplore-user-' + toString(alias.github_id),
                         alias.login_key = '__gitexplore-user-' + toString(alias.github_id)
                     WITH alias
                     OPTIONAL MATCH (alias)-[:OWNS]->(alias_repo:Repository)
                     SET alias_repo.owner_login = alias.login
                     WITH count(alias) AS alias_count
                     MERGE (viewer:User {github_id: $github_id})
                     SET viewer.login = $login,
                         viewer.login_key = $login_key,
                         viewer.name = CASE WHEN $name = '' THEN null ELSE $name END,
                         viewer.url = $url,
                         viewer.avatar_url = CASE WHEN $avatar_url = '' THEN null ELSE $avatar_url END,
                         viewer.bio = CASE WHEN $bio = '' THEN null ELSE $bio END,
                         viewer.followers_count = CASE WHEN $followers_count < 0 THEN null ELSE $followers_count END,
                         viewer.following_count = CASE WHEN $following_count < 0 THEN null ELSE $following_count END,
                         viewer.public_repositories_count = CASE WHEN $public_repositories_count < 0 THEN null ELSE $public_repositories_count END,
                         viewer.last_fetched_at = datetime($fetched_at),
                         viewer.stale_at = datetime($stale_at),
                         viewer.neighborhood_last_fetched_at = datetime($fetched_at),
                         viewer.neighborhood_stale_at = datetime($stale_at),
                         viewer.followers_complete = $followers_complete,
                         viewer.following_complete = $following_complete,
                         viewer.starred_repositories_complete = $starred_repositories_complete,
                         viewer.repositories_complete = $repositories_complete,
                          viewer.last_refresh_error = null
                     WITH viewer
                     OPTIONAL MATCH (viewer)-[:OWNS]->(viewer_repo:Repository)
                     SET viewer_repo.owner_login = viewer.login",
                )
                .param("login", viewer.login.clone())
                .param("login_key", viewer.login.to_ascii_lowercase())
                .param("github_id", viewer.github_id)
                .param("name", viewer.name.clone().unwrap_or_default())
                .param("url", viewer.url.clone())
                .param(
                    "avatar_url",
                    viewer.avatar_url.clone().unwrap_or_default(),
                )
                .param("bio", viewer.bio.clone().unwrap_or_default())
                .param(
                    "followers_count",
                    viewer.followers_count.map(|value| value as i64).unwrap_or(-1),
                )
                .param(
                    "following_count",
                    viewer.following_count.map(|value| value as i64).unwrap_or(-1),
                )
                .param(
                    "public_repositories_count",
                    viewer
                        .public_repositories_count
                        .map(|value| value as i64)
                        .unwrap_or(-1),
                )
                .param("fetched_at", fetched_at.clone())
                .param("stale_at", stale_at.clone())
                .param("followers_complete", coverage.followers_complete)
                .param("following_complete", coverage.following_complete)
                .param(
                    "starred_repositories_complete",
                    coverage.starred_repositories_complete,
                )
                .param("repositories_complete", coverage.repositories_complete),
        )
        .await?;

        if coverage.following_complete {
            run_in_transaction(
                &mut transaction,
                query(
                    "MATCH (viewer:User {github_id: $viewer_id})-[relationship:FOLLOWS]->()
                     DELETE relationship",
                )
                .param("viewer_id", viewer.github_id),
            )
            .await?;
        }
        if coverage.followers_complete {
            run_in_transaction(
                &mut transaction,
                query(
                    "MATCH ()-[relationship:FOLLOWS]->(viewer:User {github_id: $viewer_id})
                     DELETE relationship",
                )
                .param("viewer_id", viewer.github_id),
            )
            .await?;
        }
        if coverage.starred_repositories_complete {
            run_in_transaction(
                &mut transaction,
                query(
                    "MATCH (viewer:User {github_id: $viewer_id})-[relationship:STARRED]->()
                     DELETE relationship",
                )
                .param("viewer_id", viewer.github_id),
            )
            .await?;
        }
        if coverage.repositories_complete {
            run_in_transaction(
                &mut transaction,
                query(
                    "MATCH (viewer:User {github_id: $viewer_id})-[relationship:OWNS|MEMBER_OF]->()
                     DELETE relationship",
                )
                .param("viewer_id", viewer.github_id),
            )
            .await?;
        }

        for user in &import.followers {
            run_in_transaction(
                &mut transaction,
                    query(
                        "OPTIONAL MATCH (alias:User {login_key: $target_login_key})
                         WHERE alias.github_id <> $target_id
                         SET alias.login = '__gitexplore-user-' + toString(alias.github_id),
                             alias.login_key = '__gitexplore-user-' + toString(alias.github_id)
                         WITH alias
                         OPTIONAL MATCH (alias)-[:OWNS]->(alias_repo:Repository)
                         SET alias_repo.owner_login = alias.login
                         WITH count(alias) AS alias_count
                         MERGE (viewer:User {github_id: $viewer_id})
                         SET viewer.login = $viewer_login,
                             viewer.login_key = $viewer_login_key
                         MERGE (target:User {github_id: $target_id})
                         SET target.login = $target_login,
                             target.login_key = $target_login_key,
                             target.name = CASE WHEN $target_name = '' THEN target.name ELSE $target_name END,
                             target.url = $target_url,
                             target.avatar_url = CASE WHEN $target_avatar_url = '' THEN target.avatar_url ELSE $target_avatar_url END,
                             target.bio = CASE WHEN $target_bio = '' THEN target.bio ELSE $target_bio END,
                             target.followers_count = CASE WHEN $target_followers_count < 0 THEN target.followers_count ELSE $target_followers_count END,
                             target.following_count = CASE WHEN $target_following_count < 0 THEN target.following_count ELSE $target_following_count END,
                              target.public_repositories_count = CASE WHEN $target_public_repositories_count < 0 THEN target.public_repositories_count ELSE $target_public_repositories_count END
                         WITH viewer, target
                         OPTIONAL MATCH (target)-[:OWNS]->(target_repo:Repository)
                         SET target_repo.owner_login = target.login
                         WITH DISTINCT viewer, target
                         MERGE (target)-[:FOLLOWS]->(viewer)",
                    )
                    .param("viewer_id", viewer.github_id)
                    .param("viewer_login", viewer.login.clone())
                    .param("viewer_login_key", viewer.login.to_ascii_lowercase())
                    .param("target_login", user.login.clone())
                    .param("target_login_key", user.login.to_ascii_lowercase())
                    .param("target_id", user.github_id)
                    .param("target_name", user.name.clone().unwrap_or_default())
                    .param("target_url", user.url.clone())
                    .param(
                        "target_avatar_url",
                        user.avatar_url.clone().unwrap_or_default(),
                    )
                    .param("target_bio", user.bio.clone().unwrap_or_default())
                    .param(
                        "target_followers_count",
                        user.followers_count.map(|value| value as i64).unwrap_or(-1),
                    )
                    .param(
                        "target_following_count",
                        user.following_count.map(|value| value as i64).unwrap_or(-1),
                    )
                    .param(
                        "target_public_repositories_count",
                        user.public_repositories_count
                            .map(|value| value as i64)
                            .unwrap_or(-1),
                    ),
            )
            .await?;
        }

        for user in &import.following {
            run_in_transaction(
                &mut transaction,
                    query(
                        "OPTIONAL MATCH (alias:User {login_key: $target_login_key})
                         WHERE alias.github_id <> $target_id
                         SET alias.login = '__gitexplore-user-' + toString(alias.github_id),
                             alias.login_key = '__gitexplore-user-' + toString(alias.github_id)
                         WITH alias
                         OPTIONAL MATCH (alias)-[:OWNS]->(alias_repo:Repository)
                         SET alias_repo.owner_login = alias.login
                         WITH count(alias) AS alias_count
                         MERGE (viewer:User {github_id: $viewer_id})
                         SET viewer.login = $viewer_login,
                             viewer.login_key = $viewer_login_key
                         MERGE (target:User {github_id: $target_id})
                         SET target.login = $target_login,
                             target.login_key = $target_login_key,
                             target.name = CASE WHEN $target_name = '' THEN target.name ELSE $target_name END,
                             target.url = $target_url,
                             target.avatar_url = CASE WHEN $target_avatar_url = '' THEN target.avatar_url ELSE $target_avatar_url END,
                             target.bio = CASE WHEN $target_bio = '' THEN target.bio ELSE $target_bio END,
                             target.followers_count = CASE WHEN $target_followers_count < 0 THEN target.followers_count ELSE $target_followers_count END,
                             target.following_count = CASE WHEN $target_following_count < 0 THEN target.following_count ELSE $target_following_count END,
                              target.public_repositories_count = CASE WHEN $target_public_repositories_count < 0 THEN target.public_repositories_count ELSE $target_public_repositories_count END
                         WITH viewer, target
                         OPTIONAL MATCH (target)-[:OWNS]->(target_repo:Repository)
                         SET target_repo.owner_login = target.login
                         WITH DISTINCT viewer, target
                         MERGE (viewer)-[:FOLLOWS]->(target)",
                    )
                    .param("viewer_id", viewer.github_id)
                    .param("viewer_login", viewer.login.clone())
                    .param("viewer_login_key", viewer.login.to_ascii_lowercase())
                    .param("target_login", user.login.clone())
                    .param("target_login_key", user.login.to_ascii_lowercase())
                    .param("target_id", user.github_id)
                    .param("target_name", user.name.clone().unwrap_or_default())
                    .param("target_url", user.url.clone())
                    .param(
                        "target_avatar_url",
                        user.avatar_url.clone().unwrap_or_default(),
                    )
                    .param("target_bio", user.bio.clone().unwrap_or_default())
                    .param(
                        "target_followers_count",
                        user.followers_count.map(|value| value as i64).unwrap_or(-1),
                    )
                    .param(
                        "target_following_count",
                        user.following_count.map(|value| value as i64).unwrap_or(-1),
                    )
                    .param(
                        "target_public_repositories_count",
                        user.public_repositories_count
                            .map(|value| value as i64)
                            .unwrap_or(-1),
                    ),
            )
            .await?;
        }

        for repo in &import.repositories {
            run_in_transaction(
                &mut transaction,
                    query(
                        "OPTIONAL MATCH (alias:Repository {full_name_key: $full_name_key})
                         WHERE alias.github_id <> $repo_id
                         SET alias.full_name = '__gitexplore-repository-' + toString(alias.github_id),
                             alias.full_name_key = '__gitexplore-repository-' + toString(alias.github_id)
                         WITH count(alias) AS alias_count
                         MERGE (viewer:User {github_id: $viewer_id})
                         SET viewer.login = $viewer_login,
                             viewer.login_key = $viewer_login_key
                         MERGE (repo:Repository {github_id: $repo_id})
                         SET repo.full_name = $full_name,
                             repo.full_name_key = $full_name_key,
                             repo.owner_login = $owner_login,
                             repo.name = $name,
                             repo.description = CASE WHEN $description = '' THEN null ELSE $description END,
                             repo.html_url = $html_url,
                             repo.stargazer_count = $stargazer_count,
                             repo.fork_count = $fork_count,
                             repo.language = CASE WHEN $language = '' THEN null ELSE $language END,
                             repo.topics = $topics,
                             repo.pushed_at = CASE WHEN $pushed_at = '' THEN null ELSE datetime($pushed_at) END,
                             repo.updated_at = CASE WHEN $updated_at = '' THEN null ELSE datetime($updated_at) END,
                             repo.archived = $archived,
                             repo.is_fork = $is_fork,
                             repo.last_fetched_at = datetime($fetched_at),
                             repo.stale_at = datetime($stale_at),
                             repo.last_refresh_error = null
                         MERGE (viewer)-[:OWNS]->(repo)",
                    )
                    .param("viewer_id", viewer.github_id)
                    .param("viewer_login", viewer.login.clone())
                    .param("viewer_login_key", viewer.login.to_ascii_lowercase())
                    .param("full_name", repo.full_name.clone())
                    .param("full_name_key", repo.full_name.to_ascii_lowercase())
                    .param("repo_id", repo.github_id)
                    .param("owner_login", repo.owner_login.clone())
                    .param("name", repo.name.clone())
                    .param("description", repo.description.clone().unwrap_or_default())
                    .param("html_url", repo.html_url.clone())
                    .param("stargazer_count", repo.stargazer_count as i64)
                    .param("fork_count", repo.fork_count as i64)
                    .param("language", repo.language.clone().unwrap_or_default())
                    .param("topics", repo.topics.clone())
                    .param(
                        "pushed_at",
                        repo.pushed_at
                            .as_ref()
                            .map(DateTime::to_rfc3339)
                            .unwrap_or_default(),
                    )
                    .param(
                        "updated_at",
                        repo.updated_at
                            .as_ref()
                            .map(DateTime::to_rfc3339)
                            .unwrap_or_default(),
                    )
                    .param("archived", repo.archived)
                    .param("is_fork", repo.is_fork)
                    .param("fetched_at", fetched_at.clone())
                    .param("stale_at", stale_at.clone()),
            )
            .await?;
        }

        for repo in &import.starred_repositories {
            run_in_transaction(
                &mut transaction,
                    query(
                        "OPTIONAL MATCH (alias:Repository {full_name_key: $full_name_key})
                         WHERE alias.github_id <> $repo_id
                         SET alias.full_name = '__gitexplore-repository-' + toString(alias.github_id),
                             alias.full_name_key = '__gitexplore-repository-' + toString(alias.github_id)
                         WITH count(alias) AS alias_count
                         MERGE (viewer:User {github_id: $viewer_id})
                         SET viewer.login = $viewer_login,
                             viewer.login_key = $viewer_login_key
                         MERGE (repo:Repository {github_id: $repo_id})
                         SET repo.full_name = $full_name,
                             repo.full_name_key = $full_name_key,
                             repo.owner_login = $owner_login,
                             repo.name = $name,
                             repo.description = CASE WHEN $description = '' THEN null ELSE $description END,
                             repo.html_url = $html_url,
                             repo.stargazer_count = $stargazer_count,
                             repo.fork_count = $fork_count,
                             repo.language = CASE WHEN $language = '' THEN null ELSE $language END,
                             repo.topics = $topics,
                             repo.pushed_at = CASE WHEN $pushed_at = '' THEN null ELSE datetime($pushed_at) END,
                             repo.updated_at = CASE WHEN $updated_at = '' THEN null ELSE datetime($updated_at) END,
                             repo.archived = $archived,
                             repo.is_fork = $is_fork,
                             repo.last_fetched_at = datetime($fetched_at),
                             repo.stale_at = datetime($stale_at),
                             repo.last_refresh_error = null
                         MERGE (viewer)-[:STARRED]->(repo)",
                    )
                    .param("viewer_id", viewer.github_id)
                    .param("viewer_login", viewer.login.clone())
                    .param("viewer_login_key", viewer.login.to_ascii_lowercase())
                    .param("full_name", repo.full_name.clone())
                    .param("full_name_key", repo.full_name.to_ascii_lowercase())
                    .param("repo_id", repo.github_id)
                    .param("owner_login", repo.owner_login.clone())
                    .param("name", repo.name.clone())
                    .param("description", repo.description.clone().unwrap_or_default())
                    .param("html_url", repo.html_url.clone())
                    .param("stargazer_count", repo.stargazer_count as i64)
                    .param("fork_count", repo.fork_count as i64)
                    .param("language", repo.language.clone().unwrap_or_default())
                    .param("topics", repo.topics.clone())
                    .param(
                        "pushed_at",
                        repo.pushed_at
                            .as_ref()
                            .map(DateTime::to_rfc3339)
                            .unwrap_or_default(),
                    )
                    .param(
                        "updated_at",
                        repo.updated_at
                            .as_ref()
                            .map(DateTime::to_rfc3339)
                            .unwrap_or_default(),
                    )
                    .param("archived", repo.archived)
                    .param("is_fork", repo.is_fork)
                    .param("fetched_at", fetched_at.clone())
                    .param("stale_at", stale_at.clone()),
            )
            .await?;
        }

        let summary = SyncSummary {
            followers: import.followers.len(),
            following: import.following.len(),
            starred_repositories: import.starred_repositories.len(),
            repositories: import.repositories.len(),
            synced_at: Utc::now(),
            coverage,
        };
        if let Some((lease, canonical_login)) = refresh {
            let outcome_json = serde_json::to_string(&UserRefreshOutcome {
                canonical_login: canonical_login.to_string(),
                summary: summary.clone(),
            })?;
            run_in_transaction(
                &mut transaction,
                query(
                    "MATCH (lease:RefreshLease {entity_key: $entity_key})
                     WHERE lease.status = 'running' AND lease.token = $token
                     SET lease.status = 'succeeded',
                         lease.completed_at = datetime(),
                         lease.expires_at = null,
                         lease.outcome_json = $outcome_json,
                         lease.last_error = null",
                )
                .param("entity_key", lease.entity_key.clone())
                .param("token", lease.token.clone())
                .param("outcome_json", outcome_json),
            )
            .await?;
        }

        transaction
            .commit()
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        Ok(summary)
    }

    async fn resolve_bookmark_target(&self, target: &BookmarkTarget) -> AppResult<()> {
        let q = match target {
            BookmarkTarget::GitHubUser { login } => query(
                "OPTIONAL MATCH (target:User {login_key: $login_key})
                 RETURN count(target) > 0 AS found",
            )
            .param("login_key", login.to_ascii_lowercase()),
            BookmarkTarget::GitHubRepository { full_name } => query(
                "OPTIONAL MATCH (repo:Repository {full_name_key: $full_name_key})
                 RETURN count(repo) > 0 AS found",
            )
            .param("full_name_key", full_name.to_ascii_lowercase()),
        };

        let mut rows = self
            .client
            .graph
            .execute_on(&self.client.database, q)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let found = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .and_then(|row| row.get::<bool>("found").ok())
            .unwrap_or(false);

        if found {
            Ok(())
        } else {
            Err(AppError::Validation(
                "bookmark target is missing from the imported GitHub graph; run sync first"
                    .to_string(),
            ))
        }
    }
}

#[async_trait]
impl GitHubImportRepository for Neo4jGitHubImportRepository {
    async fn import_github_graph(
        &self,
        user_id: &str,
        import: GraphImport,
    ) -> AppResult<SyncSummary> {
        self.import_github_graph_inner(user_id, import, None).await
    }

    async fn try_acquire_refresh_lease(
        &self,
        entity_key: &str,
        token: &str,
        lease_seconds: i64,
    ) -> AppResult<RefreshLeaseAttempt> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MERGE (lease:RefreshLease {entity_key: $entity_key})
                     ON CREATE SET lease.status = 'idle', lease.mutex = 0
                     SET lease.mutex = coalesce(lease.mutex, 0) + 1
                     WITH lease,
                          coalesce(lease.status, 'idle') <> 'running'
                            OR lease.expires_at IS NULL
                            OR lease.expires_at <= datetime() AS available
                     FOREACH (_ IN CASE WHEN available THEN [1] ELSE [] END |
                       SET lease.status = 'running',
                           lease.token = $token,
                           lease.started_at = datetime(),
                           lease.expires_at = datetime() + duration({seconds: $lease_seconds}),
                           lease.completed_at = null,
                           lease.outcome_json = null,
                           lease.last_error = null
                     )
                     RETURN available AS acquired,
                            lease.status AS status,
                            lease.token AS token,
                            toString(lease.expires_at) AS expires_at,
                            lease.expires_at IS NOT NULL AND lease.expires_at <= datetime() AS expired,
                            lease.outcome_json AS outcome_json,
                            lease.last_error AS last_error",
                )
                .param("entity_key", entity_key.to_string())
                .param("token", token.to_string())
                .param("lease_seconds", lease_seconds),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let row = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .ok_or_else(|| AppError::Storage("refresh lease query returned no row".to_string()))?;
        if row.get::<bool>("acquired").map_err(map_neo4j_decode)? {
            let expires_at = optional_timestamp(&row, "expires_at")?.ok_or_else(|| {
                AppError::Storage("acquired refresh lease has no expiry".to_string())
            })?;
            return Ok(RefreshLeaseAttempt::Acquired(RefreshLease {
                entity_key: entity_key.to_string(),
                token: token.to_string(),
                expires_at,
            }));
        }
        Ok(RefreshLeaseAttempt::Busy(decode_refresh_lease_state(&row)?))
    }

    async fn renew_refresh_lease(
        &self,
        lease: &RefreshLease,
        lease_seconds: i64,
    ) -> AppResult<bool> {
        self.mutate_refresh_lease(
            query(
                "MATCH (lease:RefreshLease {entity_key: $entity_key})
                 SET lease.mutex = coalesce(lease.mutex, 0) + 1
                 WITH lease,
                      lease.status = 'running'
                        AND lease.token = $token
                        AND lease.expires_at > datetime() AS valid
                 FOREACH (_ IN CASE WHEN valid THEN [1] ELSE [] END |
                   SET lease.expires_at = datetime() + duration({seconds: $lease_seconds})
                 )
                 RETURN valid AS updated",
            )
            .param("entity_key", lease.entity_key.clone())
            .param("token", lease.token.clone())
            .param("lease_seconds", lease_seconds),
        )
        .await
    }

    async fn refresh_lease_state(&self, entity_key: &str) -> AppResult<Option<RefreshLeaseState>> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (lease:RefreshLease {entity_key: $entity_key})
                     RETURN lease.status AS status,
                            lease.token AS token,
                            toString(lease.expires_at) AS expires_at,
                            lease.expires_at IS NOT NULL AND lease.expires_at <= datetime() AS expired,
                            lease.outcome_json AS outcome_json,
                            lease.last_error AS last_error",
                )
                .param("entity_key", entity_key.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        rows.next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .map(|row| decode_refresh_lease_state(&row))
            .transpose()
    }

    async fn complete_refresh_lease(
        &self,
        lease: &RefreshLease,
        outcome_json: Option<&str>,
    ) -> AppResult<bool> {
        self.mutate_refresh_lease(
            query(
                "MATCH (lease:RefreshLease {entity_key: $entity_key})
                 SET lease.mutex = coalesce(lease.mutex, 0) + 1
                 WITH lease,
                      lease.status = 'running'
                        AND lease.token = $token
                        AND lease.expires_at > datetime() AS valid
                 FOREACH (_ IN CASE WHEN valid THEN [1] ELSE [] END |
                   SET lease.status = 'succeeded',
                       lease.completed_at = datetime(),
                       lease.expires_at = null,
                       lease.outcome_json = $outcome_json,
                       lease.last_error = null
                 )
                 RETURN valid AS updated",
            )
            .param("entity_key", lease.entity_key.clone())
            .param("token", lease.token.clone())
            .param("outcome_json", outcome_json.map(ToString::to_string)),
        )
        .await
    }

    async fn fail_refresh_lease(&self, lease: &RefreshLease, error: &str) -> AppResult<bool> {
        let error = error.chars().take(1000).collect::<String>();
        self.mutate_refresh_lease(
            query(
                "MATCH (lease:RefreshLease {entity_key: $entity_key})
                 SET lease.mutex = coalesce(lease.mutex, 0) + 1
                 WITH lease, lease.status = 'running' AND lease.token = $token AS valid
                 FOREACH (_ IN CASE WHEN valid THEN [1] ELSE [] END |
                   SET lease.status = 'failed',
                       lease.completed_at = datetime(),
                       lease.expires_at = null,
                       lease.outcome_json = null,
                       lease.last_error = $error
                 )
                 RETURN valid AS updated",
            )
            .param("entity_key", lease.entity_key.clone())
            .param("token", lease.token.clone())
            .param("error", error),
        )
        .await
    }

    async fn import_github_graph_under_lease(
        &self,
        user_id: &str,
        import: GraphImport,
        lease: &RefreshLease,
        canonical_login: &str,
    ) -> AppResult<SyncSummary> {
        self.import_github_graph_inner(user_id, import, Some((lease, canonical_login)))
            .await
    }

    async fn resolve_bookmark_target(&self, target: &BookmarkTarget) -> AppResult<()> {
        Neo4jGitHubImportRepository::resolve_bookmark_target(self, target).await
    }
}

impl Neo4jGitHubImportRepository {
    async fn mutate_refresh_lease(&self, q: neo4rs::Query) -> AppResult<bool> {
        let mut rows = self
            .client
            .graph
            .execute_on(&self.client.database, q)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(false);
        };
        row.get::<bool>("updated").map_err(map_neo4j_decode)
    }
}

fn decode_refresh_lease_state(row: &Row) -> AppResult<RefreshLeaseState> {
    let status = match row
        .get::<String>("status")
        .map_err(map_neo4j_decode)?
        .as_str()
    {
        "running" => RefreshLeaseStatus::Running,
        "succeeded" => RefreshLeaseStatus::Succeeded,
        "failed" => RefreshLeaseStatus::Failed,
        other => {
            return Err(AppError::Storage(format!(
                "unknown refresh lease status `{other}`"
            )));
        }
    };
    Ok(RefreshLeaseState {
        status,
        token: row.get::<String>("token").map_err(map_neo4j_decode)?,
        expires_at: optional_timestamp(row, "expires_at")?,
        expired: row.get::<bool>("expired").map_err(map_neo4j_decode)?,
        outcome_json: optional_string(row, "outcome_json")?,
        last_error: optional_string(row, "last_error")?,
    })
}

#[async_trait]
impl SyncStateRepository for Neo4jSyncStateRepository {
    async fn sync_status(&self, user_id: &str) -> AppResult<SyncStatus> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (sync:SyncState {user_id: $user_id})
                     RETURN sync.state AS state,
                            toString(sync.last_synced_at) AS last_synced_at,
                            sync.last_error AS last_error
                     LIMIT 1",
                )
                .param("user_id", user_id.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(SyncStatus::default());
        };

        let state = parse_sync_state(&row.get::<String>("state").map_err(map_neo4j_decode)?);
        let last_synced_at = row
            .get::<Option<String>>("last_synced_at")
            .map_err(map_neo4j_decode)?
            .and_then(|value| parse_timestamp(&value));
        let last_error = row
            .get::<Option<String>>("last_error")
            .map_err(map_neo4j_decode)?;

        Ok(SyncStatus {
            state,
            last_synced_at,
            last_error,
        })
    }

    async fn set_sync_status(&self, user_id: &str, status: SyncStatus) -> AppResult<()> {
        let last_error = status.last_error.unwrap_or_default();
        let last_synced_at = status
            .last_synced_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_default();
        self.client
            .run(
                query(
                    "MERGE (sync:SyncState {user_id: $user_id})
                     SET sync.state = $state,
                         sync.last_error = CASE WHEN $last_error = '' THEN null ELSE $last_error END,
                         sync.last_synced_at = CASE WHEN $last_synced_at = '' THEN null ELSE datetime($last_synced_at) END",
                )
                .param("user_id", user_id.to_string())
                .param("state", format!("{:?}", status.state))
                .param("last_error", last_error)
                .param("last_synced_at", last_synced_at),
            )
            .await
    }
}

fn parse_sync_state(value: &str) -> crate::graph::SyncState {
    match value {
        "SyncInProgress" => crate::graph::SyncState::SyncInProgress,
        "SyncSucceeded" => crate::graph::SyncState::SyncSucceeded,
        "SyncFailed" => crate::graph::SyncState::SyncFailed,
        _ => crate::graph::SyncState::NeverSynced,
    }
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn map_neo4j_decode(error: neo4rs::DeError) -> AppError {
    AppError::External(error.to_string())
}

fn usize_to_i64(value: usize, field: &str) -> AppResult<i64> {
    i64::try_from(value).map_err(|_| AppError::Storage(format!("{field} is too large for Neo4j")))
}

fn non_negative_i64_to_usize(value: i64, field: &str) -> AppResult<usize> {
    usize::try_from(value).map_err(|_| AppError::Storage(format!("{field} must be non-negative")))
}

#[async_trait]
impl CategoryRepository for Neo4jCategoryRepository {
    async fn create_category(&self, user_id: &str, category: Category) -> AppResult<()> {
        self.client
            .run(
                query(
                    "MERGE (local:LocalUser {id: $user_id})
                     MERGE (category:Category {user_id: $user_id, name: $name})
                     SET category.description = $description
                     MERGE (local)-[:OWNS_CATEGORY]->(category)",
                )
                .param("user_id", user_id.to_string())
                .param("name", category.name)
                .param("description", category.description.unwrap_or_default()),
            )
            .await
    }

    async fn list_categories(&self, user_id: &str) -> AppResult<Vec<Category>> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (:LocalUser {id: $user_id})-[:OWNS_CATEGORY]->(category:Category)
                     RETURN category.name AS name, category.description AS description
                     ORDER BY category.name ASC",
                )
                .param("user_id", user_id.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let mut categories = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            categories.push(Category {
                name: row.get::<String>("name").map_err(map_neo4j_decode)?,
                description: row
                    .get::<Option<String>>("description")
                    .map_err(map_neo4j_decode)?
                    .filter(|value| !value.is_empty()),
            });
        }

        Ok(categories)
    }
}

#[async_trait]
impl BookmarkRepository for Neo4jBookmarkRepository {
    async fn add_bookmark(&self, user_id: &str, bookmark: Bookmark) -> AppResult<Bookmark> {
        let (target_kind, target_value) = match &bookmark.target {
            BookmarkTarget::GitHubUser { login } => ("github-user", login.to_ascii_lowercase()),
            BookmarkTarget::GitHubRepository { full_name } => {
                ("github-repository", full_name.to_ascii_lowercase())
            }
        };
        let mut transaction = self
            .client
            .graph
            .start_txn_on(self.client.database.clone())
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let bookmark_query = match &bookmark.target {
            BookmarkTarget::GitHubUser { .. } => query(
                "MATCH (target:User {login_key: $target_value})
                 MERGE (local:LocalUser {id: $user_id})
                 MERGE (bookmark:Bookmark {
                     user_id: $user_id,
                     target_kind: $target_kind,
                     target_github_id: target.github_id
                 })
                 ON CREATE SET bookmark.id = $bookmark_id,
                               bookmark.note = CASE WHEN $note = '' THEN null ELSE $note END,
                               bookmark.created_at = datetime($created_at)
                 MERGE (local)-[:CREATED_BOOKMARK]->(bookmark)
                 MERGE (bookmark)-[:TARGETS_USER]->(target)
                 RETURN bookmark.id AS id",
            ),
            BookmarkTarget::GitHubRepository { .. } => query(
                "MATCH (target:Repository {full_name_key: $target_value})
                 MERGE (local:LocalUser {id: $user_id})
                 MERGE (bookmark:Bookmark {
                     user_id: $user_id,
                     target_kind: $target_kind,
                     target_github_id: target.github_id
                 })
                 ON CREATE SET bookmark.id = $bookmark_id,
                               bookmark.note = CASE WHEN $note = '' THEN null ELSE $note END,
                               bookmark.created_at = datetime($created_at)
                 MERGE (local)-[:CREATED_BOOKMARK]->(bookmark)
                 MERGE (bookmark)-[:TARGETS_REPO]->(target)
                 RETURN bookmark.id AS id",
            ),
        }
        .param("user_id", user_id.to_string())
        .param("target_kind", target_kind)
        .param("target_value", target_value)
        .param("bookmark_id", bookmark.id.clone())
        .param("note", bookmark.note.clone().unwrap_or_default())
        .param("created_at", bookmark.created_at.to_rfc3339());
        let mut rows = transaction
            .execute(bookmark_query)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let stored_id = rows
            .next(&mut transaction)
            .await
            .map_err(|error| AppError::External(error.to_string()))?
            .ok_or_else(|| AppError::NotFound("bookmark target no longer exists".to_string()))?
            .get::<String>("id")
            .map_err(map_neo4j_decode)?;
        drop(rows);
        let created = stored_id == bookmark.id;

        if created {
            for category in &bookmark.categories {
                run_in_transaction(
                    &mut transaction,
                    query(
                        "MERGE (local:LocalUser {id: $user_id})
                         MERGE (category:Category {user_id: $user_id, name: $category_name})
                         MERGE (local)-[:OWNS_CATEGORY]->(category)
                         WITH category
                         MATCH (bookmark:Bookmark {id: $bookmark_id})
                         MERGE (bookmark)-[:IN_CATEGORY]->(category)",
                    )
                    .param("user_id", user_id.to_string())
                    .param("category_name", category.clone())
                    .param("bookmark_id", bookmark.id.clone()),
                )
                .await?;
            }
        }

        transaction
            .commit()
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        if created {
            return Ok(bookmark);
        }

        self.list_bookmarks(user_id)
            .await?
            .into_iter()
            .find(|existing| bookmark_targets_match(&existing.target, &bookmark.target))
            .ok_or_else(|| AppError::Storage("existing bookmark could not be read".to_string()))
    }

    async fn list_bookmarks(&self, user_id: &str) -> AppResult<Vec<Bookmark>> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (:LocalUser {id: $user_id})-[:CREATED_BOOKMARK]->(bookmark:Bookmark)
                     OPTIONAL MATCH (bookmark)-[:IN_CATEGORY]->(category:Category)
                     OPTIONAL MATCH (bookmark)-[:TARGETS_USER]->(user:User)
                     OPTIONAL MATCH (bookmark)-[:TARGETS_REPO]->(repo:Repository)
                     RETURN bookmark.id AS id,
                            bookmark.note AS note,
                            toString(bookmark.created_at) AS created_at,
                            collect(DISTINCT category.name) AS categories,
                            user.login AS user_login,
                            repo.full_name AS repo_full_name
                     ORDER BY bookmark.created_at DESC",
                )
                .param("user_id", user_id.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let mut bookmarks = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            let user_login = row
                .get::<Option<String>>("user_login")
                .map_err(map_neo4j_decode)?;
            let repo_full_name = row
                .get::<Option<String>>("repo_full_name")
                .map_err(map_neo4j_decode)?;
            let target = if let Some(login) = user_login.filter(|value| !value.is_empty()) {
                BookmarkTarget::GitHubUser { login }
            } else if let Some(full_name) = repo_full_name.filter(|value| !value.is_empty()) {
                BookmarkTarget::GitHubRepository { full_name }
            } else {
                return Err(AppError::External(
                    "bookmark row is missing a target relationship".to_string(),
                ));
            };

            bookmarks.push(Bookmark {
                id: row.get::<String>("id").map_err(map_neo4j_decode)?,
                target,
                categories: dedupe_strings(
                    row.get::<Vec<String>>("categories")
                        .map_err(map_neo4j_decode)?,
                ),
                note: row
                    .get::<Option<String>>("note")
                    .map_err(map_neo4j_decode)?
                    .filter(|value| !value.is_empty()),
                created_at: parse_required_timestamp(
                    &row.get::<String>("created_at").map_err(map_neo4j_decode)?,
                )?,
            });
        }

        Ok(bookmarks)
    }
}

#[async_trait]
impl ExplorationRepository for Neo4jExplorationRepository {
    async fn explore(&self, user_id: &str, seed: ExplorationSeed) -> AppResult<ExplorationResult> {
        let (mut related_people, mut related_repositories, cache_status, last_fetched_at) =
            match &seed {
                ExplorationSeed::User { login } => {
                    let (people, repos, fetched_at, stale_at) = self.user_projection(login).await?;
                    let (cache_status, _, _) = cache_status_from_metadata(CacheMetadata {
                        last_fetched_at: fetched_at,
                        stale_at,
                        refresh_started_at: None,
                        last_refresh_error: None,
                    });
                    (
                        people.into_iter().filter(|item| item != login).collect(),
                        repos,
                        cache_status,
                        fetched_at,
                    )
                }
                ExplorationSeed::Repository { full_name } => {
                    let (people, repos, fetched_at, stale_at) =
                        self.repository_projection(full_name).await?;
                    let (cache_status, _, _) = cache_status_from_metadata(CacheMetadata {
                        last_fetched_at: fetched_at,
                        stale_at,
                        refresh_started_at: None,
                        last_refresh_error: None,
                    });
                    (
                        people,
                        repos.into_iter().filter(|item| item != full_name).collect(),
                        cache_status,
                        fetched_at,
                    )
                }
                ExplorationSeed::Category { name } => {
                    let (people, repos) = self.category_projection(user_id, name).await?;
                    (people, repos, CacheStatus::Fresh, None)
                }
            };

        related_people = dedupe_strings(related_people);
        related_repositories = dedupe_strings(related_repositories);

        let snapshot = ExplorationSnapshot {
            id: Uuid::new_v4().to_string(),
            seed: seed.clone(),
            discovered_people: related_people.clone(),
            discovered_repositories: related_repositories.clone(),
            generated_at: Utc::now(),
        };

        self.client
            .run(
                query(
                    "MERGE (local:LocalUser {id: $user_id})
                     MERGE (snapshot:ExplorationSnapshot {id: $snapshot_id})
                     SET snapshot.user_id = $user_id,
                         snapshot.seed_type = $seed_type,
                         snapshot.seed_value = $seed_value,
                         snapshot.discovered_people = $discovered_people,
                         snapshot.discovered_repositories = $discovered_repositories,
                         snapshot.generated_at = datetime($generated_at)
                     MERGE (local)-[:SAVED_SNAPSHOT]->(snapshot)",
                )
                .param("user_id", user_id.to_string())
                .param("snapshot_id", snapshot.id.clone())
                .param("seed_type", seed_type(&snapshot.seed))
                .param("seed_value", seed_value(&snapshot.seed))
                .param("discovered_people", snapshot.discovered_people.clone())
                .param(
                    "discovered_repositories",
                    snapshot.discovered_repositories.clone(),
                )
                .param("generated_at", snapshot.generated_at.to_rfc3339()),
            )
            .await?;

        Ok(ExplorationResult {
            seed,
            related_people,
            related_repositories,
            saved_snapshot: snapshot,
            cache_status,
            last_fetched_at,
            refresh_job_status: None,
            overload_message: None,
        })
    }

    async fn list_exploration_snapshots(
        &self,
        user_id: &str,
    ) -> AppResult<Vec<ExplorationSnapshot>> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (:LocalUser {id: $user_id})-[:SAVED_SNAPSHOT]->(snapshot:ExplorationSnapshot)
                     RETURN snapshot.id AS id,
                            snapshot.seed_type AS seed_type,
                            snapshot.seed_value AS seed_value,
                            snapshot.discovered_people AS discovered_people,
                            snapshot.discovered_repositories AS discovered_repositories,
                            toString(snapshot.generated_at) AS generated_at
                     ORDER BY snapshot.generated_at DESC",
                )
                .param("user_id", user_id.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let mut snapshots = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            let seed = parse_seed(
                &row.get::<String>("seed_type").map_err(map_neo4j_decode)?,
                &row.get::<String>("seed_value").map_err(map_neo4j_decode)?,
            )?;
            snapshots.push(ExplorationSnapshot {
                id: row.get::<String>("id").map_err(map_neo4j_decode)?,
                seed,
                discovered_people: dedupe_strings(
                    row.get::<Vec<String>>("discovered_people")
                        .map_err(map_neo4j_decode)?,
                ),
                discovered_repositories: dedupe_strings(
                    row.get::<Vec<String>>("discovered_repositories")
                        .map_err(map_neo4j_decode)?,
                ),
                generated_at: parse_required_timestamp(
                    &row.get::<String>("generated_at")
                        .map_err(map_neo4j_decode)?,
                )?,
            });
        }

        Ok(snapshots)
    }
}

impl Neo4jExplorationRepository {
    async fn user_projection(
        &self,
        login: &str,
    ) -> AppResult<(
        Vec<String>,
        Vec<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (viewer:User {login_key: $login_key})
                     OPTIONAL MATCH (follower:User)-[:FOLLOWS]->(viewer)
                     WITH viewer, collect(DISTINCT follower.login) AS follower_people
                     OPTIONAL MATCH (viewer)-[:FOLLOWS]->(following:User)
                     WITH viewer, follower_people, collect(DISTINCT following.login) AS following_people
                     OPTIONAL MATCH (viewer)-[:STARRED|OWNS|MEMBER_OF]->(repo:Repository)
                     RETURN follower_people AS follower_people,
                            following_people AS following_people,
                            collect(DISTINCT repo.full_name) AS repositories,
                            toString(viewer.neighborhood_last_fetched_at) AS last_fetched_at,
                            toString(viewer.neighborhood_stale_at) AS stale_at",
                )
                .param("login_key", login.to_ascii_lowercase()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Err(AppError::Validation(
                "run sync before exploring".to_string(),
            ));
        };

        let people = dedupe_strings(
            row.get::<Vec<String>>("follower_people")
                .map_err(map_neo4j_decode)?
                .into_iter()
                .chain(
                    row.get::<Vec<String>>("following_people")
                        .map_err(map_neo4j_decode)?,
                )
                .collect(),
        );
        let repositories = dedupe_strings(
            row.get::<Vec<String>>("repositories")
                .map_err(map_neo4j_decode)?,
        );
        Ok((
            people,
            repositories,
            row.get::<Option<String>>("last_fetched_at")
                .ok()
                .flatten()
                .and_then(|value| parse_timestamp(&value)),
            row.get::<Option<String>>("stale_at")
                .ok()
                .flatten()
                .and_then(|value| parse_timestamp(&value)),
        ))
    }

    async fn repository_projection(
        &self,
        full_name: &str,
    ) -> AppResult<(
        Vec<String>,
        Vec<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (repo:Repository {full_name_key: $full_name_key})
                     OPTIONAL MATCH (user:User)-[:STARRED|OWNS|MEMBER_OF]->(repo)
                     WITH repo, collect(DISTINCT user.login) AS people
                     OPTIONAL MATCH (peer:User)-[:STARRED|OWNS|MEMBER_OF]->(other:Repository)
                     WHERE peer.login IN people AND other.full_name_key <> $full_name_key
                     RETURN people AS people,
                            collect(DISTINCT other.full_name) AS repositories,
                            toString(repo.last_fetched_at) AS last_fetched_at,
                            toString(repo.stale_at) AS stale_at",
                )
                .param("full_name_key", full_name.to_ascii_lowercase()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Err(AppError::Validation(
                "run sync before exploring".to_string(),
            ));
        };

        Ok((
            dedupe_strings(row.get::<Vec<String>>("people").map_err(map_neo4j_decode)?),
            dedupe_strings(
                row.get::<Vec<String>>("repositories")
                    .map_err(map_neo4j_decode)?,
            ),
            row.get::<Option<String>>("last_fetched_at")
                .ok()
                .flatten()
                .and_then(|value| parse_timestamp(&value)),
            row.get::<Option<String>>("stale_at")
                .ok()
                .flatten()
                .and_then(|value| parse_timestamp(&value)),
        ))
    }

    async fn category_projection(
        &self,
        user_id: &str,
        name: &str,
    ) -> AppResult<(Vec<String>, Vec<String>)> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (:LocalUser {id: $user_id})-[:CREATED_BOOKMARK]->(bookmark:Bookmark)-[:IN_CATEGORY]->(:Category {user_id: $user_id, name: $name})
                     OPTIONAL MATCH (bookmark)-[:TARGETS_USER]->(user:User)
                     OPTIONAL MATCH (bookmark)-[:TARGETS_REPO]->(repo:Repository)
                     RETURN collect(DISTINCT user.login) AS people,
                            collect(DISTINCT repo.full_name) AS repositories",
                )
                .param("user_id", user_id.to_string())
                .param("name", name.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok((Vec::new(), Vec::new()));
        };

        Ok((
            dedupe_strings(row.get::<Vec<String>>("people").map_err(map_neo4j_decode)?),
            dedupe_strings(
                row.get::<Vec<String>>("repositories")
                    .map_err(map_neo4j_decode)?,
            ),
        ))
    }
}

#[async_trait]
impl DiscoveryRepository for Neo4jDiscoveryRepository {
    async fn user_neighborhood(&self, user_id: &str, login: &str) -> AppResult<UserNeighborhood> {
        let (user, coverage) = self.discovery_user(login).await?;
        let mut followers = self
            .related_users(
                query(
                    "MATCH (viewer:User)
                     WHERE viewer.login_key = $login_key
                     MATCH (related:User)-[:FOLLOWS]->(viewer)
                     RETURN related.github_id AS github_id,
                            related.login AS login,
                            related.name AS name,
                            related.url AS url,
                            related.avatar_url AS avatar_url,
                            related.bio AS bio,
                            related.followers_count AS followers_count,
                            related.following_count AS following_count,
                            related.public_repositories_count AS public_repositories_count,
                            toString(related.neighborhood_last_fetched_at) AS neighborhood_last_fetched_at,
                            toString(related.neighborhood_stale_at) AS neighborhood_stale_at
                     ORDER BY related.login ASC",
                )
                .param("login_key", login.to_ascii_lowercase()),
            )
            .await?;
        let mut following = self
            .related_users(
                query(
                    "MATCH (viewer:User)
                     WHERE viewer.login_key = $login_key
                     MATCH (viewer)-[:FOLLOWS]->(related:User)
                     RETURN related.github_id AS github_id,
                            related.login AS login,
                            related.name AS name,
                            related.url AS url,
                            related.avatar_url AS avatar_url,
                            related.bio AS bio,
                            related.followers_count AS followers_count,
                            related.following_count AS following_count,
                            related.public_repositories_count AS public_repositories_count,
                            toString(related.neighborhood_last_fetched_at) AS neighborhood_last_fetched_at,
                            toString(related.neighborhood_stale_at) AS neighborhood_stale_at
                     ORDER BY related.login ASC",
                )
                .param("login_key", login.to_ascii_lowercase()),
            )
            .await?;
        followers.dedup_by(|left, right| left.profile.login == right.profile.login);
        following.dedup_by(|left, right| left.profile.login == right.profile.login);

        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (viewer:User)
                     WHERE viewer.login_key = $login_key
                     MATCH (viewer)-[relationship:STARRED|OWNS|MEMBER_OF]->(repo:Repository)
                     RETURN repo.github_id AS github_id,
                            repo.owner_login AS owner_login,
                            repo.name AS name,
                            repo.full_name AS full_name,
                            repo.description AS description,
                            repo.html_url AS html_url,
                            repo.stargazer_count AS stargazer_count,
                            repo.fork_count AS fork_count,
                            repo.language AS language,
                            repo.topics AS topics,
                            toString(repo.pushed_at) AS pushed_at,
                            toString(repo.updated_at) AS updated_at,
                            repo.archived AS archived,
                            repo.is_fork AS is_fork,
                            type(relationship) AS relationship_kind,
                            EXISTS {
                                MATCH (:LocalUser {id: $user_id})-[:CREATED_BOOKMARK]->(:Bookmark)-[:TARGETS_REPO]->(repo)
                            } AS saved
                     ORDER BY repo.full_name ASC",
                )
                .param("login_key", login.to_ascii_lowercase())
                .param("user_id", user_id.to_string()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let mut starred_repositories = Vec::new();
        let mut owned_repositories = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            let relationship_kind = row
                .get::<String>("relationship_kind")
                .map_err(map_neo4j_decode)?;
            let record = DiscoveryRepositoryRecord {
                repository: decode_repository_node(&row)?,
                saved: row.get::<bool>("saved").map_err(map_neo4j_decode)?,
            };
            if relationship_kind == "STARRED" {
                starred_repositories.push(record);
            } else {
                owned_repositories.push(record);
            }
        }
        sort_repository_records(&mut starred_repositories);
        sort_repository_records(&mut owned_repositories);
        starred_repositories
            .dedup_by(|left, right| left.repository.github_id == right.repository.github_id);
        owned_repositories
            .dedup_by(|left, right| left.repository.github_id == right.repository.github_id);

        Ok(UserNeighborhood {
            user,
            followers,
            following,
            starred_repositories,
            owned_repositories,
            coverage,
        })
    }

    async fn discover_repositories(
        &self,
        user_id: &str,
        login: &str,
        limit: usize,
    ) -> AppResult<Vec<RepositoryCandidate>> {
        let neighborhood = self.user_neighborhood(user_id, login).await?;
        let preferred_languages = neighborhood
            .starred_repositories
            .iter()
            .chain(neighborhood.owned_repositories.iter())
            .filter_map(|record| record.repository.language.as_ref())
            .map(|language| language.to_ascii_lowercase())
            .collect::<HashSet<_>>();

        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (seed:User)
                     WHERE seed.login_key = $login_key
                     MATCH (peer:User)-[candidate_relationship:STARRED|OWNS|MEMBER_OF]->(repo:Repository)
                     WHERE (
                         EXISTS { MATCH (seed)-[:FOLLOWS]->(peer) }
                         OR EXISTS { MATCH (peer)-[:FOLLOWS]->(seed) }
                     )
                     AND NOT EXISTS {
                         MATCH (seed)-[:STARRED|OWNS|MEMBER_OF]->(repo)
                     }
                     WITH seed, repo, collect({
                         peer_login: peer.login,
                         relationship_kind: type(candidate_relationship),
                         followed: EXISTS { MATCH (seed)-[:FOLLOWS]->(peer) },
                         follower: EXISTS { MATCH (peer)-[:FOLLOWS]->(seed) }
                     }) AS signals
                     ORDER BY size(signals) DESC,
                              coalesce(repo.stargazer_count, 0) DESC,
                              repo.full_name ASC
                     LIMIT $candidate_pool_limit
                     UNWIND signals AS signal
                     RETURN repo.github_id AS github_id,
                            repo.owner_login AS owner_login,
                            repo.name AS name,
                            repo.full_name AS full_name,
                            repo.description AS description,
                            repo.html_url AS html_url,
                            repo.stargazer_count AS stargazer_count,
                            repo.fork_count AS fork_count,
                            repo.language AS language,
                            repo.topics AS topics,
                            toString(repo.pushed_at) AS pushed_at,
                            toString(repo.updated_at) AS updated_at,
                            repo.archived AS archived,
                            repo.is_fork AS is_fork,
                            signal.peer_login AS peer_login,
                            signal.relationship_kind AS relationship_kind,
                            signal.followed AS followed,
                            signal.follower AS follower,
                            EXISTS {
                                MATCH (:LocalUser {id: $user_id})-[:CREATED_BOOKMARK]->(:Bookmark)-[:TARGETS_REPO]->(repo)
                            } AS saved",
                )
                .param("login_key", login.to_ascii_lowercase())
                .param("user_id", user_id.to_string())
                .param(
                    "candidate_pool_limit",
                    limit.saturating_mul(20).clamp(200, 1_000) as i64,
                ),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;

        let mut candidates =
            HashMap::<String, (GitHubRepositoryNode, bool, CandidateSignalAccumulator)>::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            let repository = decode_repository_node(&row)?;
            let full_name = repository.full_name.clone();
            let saved = row.get::<bool>("saved").map_err(map_neo4j_decode)?;
            let peer_login = row.get::<String>("peer_login").map_err(map_neo4j_decode)?;
            let followed = row.get::<bool>("followed").map_err(map_neo4j_decode)?;
            let follower = row.get::<bool>("follower").map_err(map_neo4j_decode)?;
            let starred = row
                .get::<String>("relationship_kind")
                .map_err(map_neo4j_decode)?
                == "STARRED";
            let entry = candidates
                .entry(full_name)
                .or_insert_with(|| (repository, saved, CandidateSignalAccumulator::default()));
            entry.1 |= saved;
            entry.2.record(&peer_login, followed, follower, starred);
        }

        let mut ranked = candidates
            .into_values()
            .map(|(repository, saved, accumulator)| {
                let (graph_signals, via_logins) = accumulator.into_parts();
                rank_repository_candidate(
                    repository,
                    saved,
                    graph_signals,
                    via_logins,
                    &preferred_languages,
                    Utc::now(),
                )
            })
            .collect::<Vec<_>>();
        sort_repository_candidates(&mut ranked);
        ranked.truncate(limit);
        Ok(ranked)
    }
}

#[async_trait]
impl InsightRepository for Neo4jInsightRepository {
    async fn repository_contributors(
        &self,
        full_name: &str,
    ) -> AppResult<Option<RepositoryContributorInsights>> {
        let mut metadata_rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (repo:Repository {full_name_key: $full_name_key})
                     WHERE repo.contributors_last_fetched_at IS NOT NULL
                     RETURN repo.full_name AS full_name,
                            coalesce(repo.contributors_source_complete, false) AS source_complete,
                            toString(repo.contributors_last_fetched_at) AS last_fetched_at,
                            toString(repo.contributors_stale_at) AS stale_at,
                            toString(repo.contributors_refresh_started_at) AS refresh_started_at,
                            repo.contributors_last_refresh_error AS last_refresh_error
                     LIMIT 1",
                )
                .param("full_name_key", full_name.to_ascii_lowercase()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(metadata_row) = metadata_rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(None);
        };

        let canonical = metadata_row
            .get::<String>("full_name")
            .map_err(map_neo4j_decode)?;
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (contributor:User)-[activity:CONTRIBUTED_TO]->(repo:Repository {full_name_key: $full_name_key})
                     RETURN contributor.github_id AS github_id,
                            contributor.login AS login,
                            contributor.avatar_url AS avatar_url,
                            contributor.url AS url,
                            activity.contributions AS contributions
                     ORDER BY activity.contributions DESC, contributor.login ASC",
                )
                .param("full_name_key", full_name.to_ascii_lowercase()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let mut contributors = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            contributors.push(RepositoryContributor {
                github_id: row.get::<i64>("github_id").map_err(map_neo4j_decode)?,
                login: row.get::<String>("login").map_err(map_neo4j_decode)?,
                avatar_url: optional_string(&row, "avatar_url")?,
                url: row.get::<String>("url").map_err(map_neo4j_decode)?,
                contributions: optional_u64(&row, "contributions")?.unwrap_or_default(),
            });
        }
        Ok(Some(RepositoryContributorInsights {
            full_name: canonical,
            contributors,
            source_complete: metadata_row
                .get::<bool>("source_complete")
                .map_err(map_neo4j_decode)?,
            cache: decode_insight_cache(&metadata_row)?,
        }))
    }

    async fn user_commit_repositories(
        &self,
        login: &str,
    ) -> AppResult<Option<UserCommitRepositoryInsights>> {
        let mut metadata_rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (user:User {login_key: $login_key})
                     WHERE user.commit_activity_last_fetched_at IS NOT NULL
                     RETURN user.login AS login,
                            coalesce(user.commit_activity_source_event_count, 0) AS source_event_count,
                            coalesce(user.commit_activity_source_truncated, false) AS source_truncated,
                            toString(user.commit_activity_last_fetched_at) AS last_fetched_at,
                            toString(user.commit_activity_stale_at) AS stale_at,
                            toString(user.commit_activity_refresh_started_at) AS refresh_started_at,
                            user.commit_activity_last_refresh_error AS last_refresh_error
                     LIMIT 1",
                )
                .param("login_key", login.to_ascii_lowercase()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(metadata_row) = metadata_rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(None);
        };

        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (user:User {login_key: $login_key})-[activity:RECENTLY_PUSHED_TO]->(repo:Repository)
                     RETURN repo.github_id AS github_id,
                            repo.full_name AS full_name,
                            repo.html_url AS url,
                            activity.push_count AS push_count,
                            activity.commit_count AS commit_count,
                            toString(activity.last_pushed_at) AS last_pushed_at
                     ORDER BY activity.commit_count DESC,
                              activity.push_count DESC,
                              activity.last_pushed_at DESC,
                              repo.full_name ASC",
                )
                .param("login_key", login.to_ascii_lowercase()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let mut repositories = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            let last_pushed_at = row
                .get::<String>("last_pushed_at")
                .map_err(map_neo4j_decode)?;
            repositories.push(UserCommitRepository {
                github_id: row.get::<i64>("github_id").map_err(map_neo4j_decode)?,
                full_name: row.get::<String>("full_name").map_err(map_neo4j_decode)?,
                url: row.get::<String>("url").map_err(map_neo4j_decode)?,
                push_count: optional_u64(&row, "push_count")?.unwrap_or_default(),
                commit_count: optional_u64(&row, "commit_count")?.unwrap_or_default(),
                last_pushed_at: parse_required_timestamp(&last_pushed_at)?,
            });
        }
        Ok(Some(UserCommitRepositoryInsights {
            login: metadata_row
                .get::<String>("login")
                .map_err(map_neo4j_decode)?,
            repositories,
            source_event_count: metadata_row
                .get::<i64>("source_event_count")
                .map_err(map_neo4j_decode)?
                .try_into()
                .unwrap_or_default(),
            source_truncated: metadata_row
                .get::<bool>("source_truncated")
                .map_err(map_neo4j_decode)?,
            cache: decode_insight_cache(&metadata_row)?,
        }))
    }

    async fn begin_repository_contributor_refresh(&self, full_name: &str) -> AppResult<bool> {
        self.begin_refresh(
            query(
                "MATCH (repo:Repository {full_name_key: $key})
                 WHERE repo.contributors_last_fetched_at IS NOT NULL
                   AND (repo.contributors_refresh_started_at IS NULL OR repo.contributors_refresh_started_at <= datetime($refresh_cutoff))
                   AND (repo.contributors_last_refresh_error IS NULL OR repo.contributors_stale_at <= datetime($now))
                 SET repo.contributors_refresh_started_at = datetime($now),
                     repo.contributors_last_refresh_error = null
                 RETURN count(repo) AS updated",
            )
            .param("key", full_name.to_ascii_lowercase())
            .param("now", Utc::now().to_rfc3339())
            .param(
                "refresh_cutoff",
                (Utc::now() - Duration::minutes(INSIGHT_REFRESH_TIMEOUT_MINUTES)).to_rfc3339(),
            ),
        )
        .await
    }

    async fn begin_user_commit_repository_refresh(&self, login: &str) -> AppResult<bool> {
        self.begin_refresh(
            query(
                "MATCH (user:User {login_key: $key})
                 WHERE user.commit_activity_last_fetched_at IS NOT NULL
                   AND (user.commit_activity_refresh_started_at IS NULL OR user.commit_activity_refresh_started_at <= datetime($refresh_cutoff))
                   AND (user.commit_activity_last_refresh_error IS NULL OR user.commit_activity_stale_at <= datetime($now))
                 SET user.commit_activity_refresh_started_at = datetime($now),
                     user.commit_activity_last_refresh_error = null
                 RETURN count(user) AS updated",
            )
            .param("key", login.to_ascii_lowercase())
            .param("now", Utc::now().to_rfc3339())
            .param(
                "refresh_cutoff",
                (Utc::now() - Duration::minutes(INSIGHT_REFRESH_TIMEOUT_MINUTES)).to_rfc3339(),
            ),
        )
        .await
    }

    async fn save_repository_contributors(
        &self,
        insights: RepositoryContributorInsights,
    ) -> AppResult<()> {
        let mut transaction = self
            .client
            .graph
            .start_txn_on(self.client.database.clone())
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        run_in_transaction(
            &mut transaction,
            query(
                "MATCH (repo:Repository {full_name_key: $full_name_key})
                 SET repo.contributors_last_fetched_at = datetime($last_fetched_at),
                     repo.contributors_stale_at = datetime($stale_at),
                     repo.contributors_refresh_started_at = null,
                     repo.contributors_last_refresh_error = null,
                     repo.contributors_source_complete = $source_complete",
            )
            .param("full_name_key", insights.full_name.to_ascii_lowercase())
            .param(
                "last_fetched_at",
                required_cache_timestamp(insights.cache.last_fetched_at, "last_fetched_at")?,
            )
            .param(
                "stale_at",
                required_cache_timestamp(insights.cache.stale_at, "stale_at")?,
            )
            .param("source_complete", insights.source_complete),
        )
        .await?;
        run_in_transaction(
            &mut transaction,
            query(
                "MATCH ()-[activity:CONTRIBUTED_TO]->(repo:Repository {full_name_key: $full_name_key})
                 DELETE activity",
            )
            .param("full_name_key", insights.full_name.to_ascii_lowercase()),
        )
        .await?;
        for contributor in insights.contributors {
            run_in_transaction(
                &mut transaction,
                query(
                    "OPTIONAL MATCH (alias:User {login_key: $login_key})
                     WHERE alias.github_id <> $github_id
                     SET alias.login = '__gitexplore-user-' + toString(alias.github_id),
                         alias.login_key = '__gitexplore-user-' + toString(alias.github_id)
                     WITH alias
                     OPTIONAL MATCH (alias)-[:OWNS]->(alias_repo:Repository)
                     SET alias_repo.owner_login = alias.login
                     WITH count(alias) AS alias_count
                     MERGE (contributor:User {github_id: $github_id})
                     SET contributor.login = $login,
                         contributor.login_key = $login_key,
                         contributor.url = $url,
                         contributor.avatar_url = CASE WHEN $avatar_url = '' THEN contributor.avatar_url ELSE $avatar_url END
                     WITH contributor
                     OPTIONAL MATCH (contributor)-[:OWNS]->(owned:Repository)
                     SET owned.owner_login = contributor.login
                     WITH DISTINCT contributor
                     MATCH (repo:Repository {full_name_key: $full_name_key})
                     MERGE (contributor)-[activity:CONTRIBUTED_TO]->(repo)
                     SET activity.contributions = $contributions",
                )
                .param("full_name_key", insights.full_name.to_ascii_lowercase())
                .param("github_id", contributor.github_id)
                .param("login", contributor.login.clone())
                .param("login_key", contributor.login.to_ascii_lowercase())
                .param("url", contributor.url)
                .param("avatar_url", contributor.avatar_url.unwrap_or_default())
                .param("contributions", contributor.contributions as i64),
            )
            .await?;
        }
        transaction
            .commit()
            .await
            .map_err(|error| AppError::External(error.to_string()))
    }

    async fn save_user_commit_repositories(
        &self,
        insights: UserCommitRepositoryInsights,
    ) -> AppResult<()> {
        let mut transaction = self
            .client
            .graph
            .start_txn_on(self.client.database.clone())
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        run_in_transaction(
            &mut transaction,
            query(
                "MATCH (user:User {login_key: $login_key})
                 SET user.commit_activity_last_fetched_at = datetime($last_fetched_at),
                     user.commit_activity_stale_at = datetime($stale_at),
                     user.commit_activity_refresh_started_at = null,
                     user.commit_activity_last_refresh_error = null,
                     user.commit_activity_source_event_count = $source_event_count,
                     user.commit_activity_source_truncated = $source_truncated",
            )
            .param("login_key", insights.login.to_ascii_lowercase())
            .param(
                "last_fetched_at",
                required_cache_timestamp(insights.cache.last_fetched_at, "last_fetched_at")?,
            )
            .param(
                "stale_at",
                required_cache_timestamp(insights.cache.stale_at, "stale_at")?,
            )
            .param("source_event_count", insights.source_event_count as i64)
            .param("source_truncated", insights.source_truncated),
        )
        .await?;
        run_in_transaction(
            &mut transaction,
            query(
                "MATCH (user:User {login_key: $login_key})-[activity:RECENTLY_PUSHED_TO]->()
                 DELETE activity",
            )
            .param("login_key", insights.login.to_ascii_lowercase()),
        )
        .await?;
        for recent in insights.repositories {
            let (owner_login, name) = recent.full_name.split_once('/').ok_or_else(|| {
                AppError::External(format!(
                    "GitHub event returned invalid repository name `{}`",
                    recent.full_name
                ))
            })?;
            run_in_transaction(
                &mut transaction,
                query(
                    "OPTIONAL MATCH (alias:Repository {full_name_key: $full_name_key})
                     WHERE alias.github_id <> $github_id
                     SET alias.full_name = '__gitexplore-repository-' + toString(alias.github_id),
                         alias.full_name_key = '__gitexplore-repository-' + toString(alias.github_id)
                     WITH count(alias) AS alias_count
                     MATCH (user:User {login_key: $login_key})
                     MERGE (repo:Repository {github_id: $github_id})
                     SET repo.full_name = $full_name,
                         repo.full_name_key = $full_name_key,
                         repo.owner_login = $owner_login,
                         repo.name = $name,
                         repo.html_url = CASE WHEN coalesce(repo.html_url, '') = '' THEN $url ELSE repo.html_url END
                     MERGE (user)-[activity:RECENTLY_PUSHED_TO]->(repo)
                     SET activity.push_count = $push_count,
                         activity.commit_count = $commit_count,
                         activity.last_pushed_at = datetime($last_pushed_at)",
                )
                .param("login_key", insights.login.to_ascii_lowercase())
                .param("github_id", recent.github_id)
                .param("full_name", recent.full_name.clone())
                .param("full_name_key", recent.full_name.to_ascii_lowercase())
                .param("owner_login", owner_login.to_string())
                .param("name", name.to_string())
                .param("url", recent.url)
                .param("push_count", recent.push_count as i64)
                .param("commit_count", recent.commit_count as i64)
                .param("last_pushed_at", recent.last_pushed_at.to_rfc3339()),
            )
            .await?;
        }
        transaction
            .commit()
            .await
            .map_err(|error| AppError::External(error.to_string()))
    }

    async fn fail_repository_contributor_refresh(
        &self,
        full_name: &str,
        error: &str,
    ) -> AppResult<()> {
        self.client
            .run(
                query(
                    "MATCH (repo:Repository {full_name_key: $key})
                     SET repo.contributors_refresh_started_at = null,
                         repo.contributors_last_refresh_error = $error,
                         repo.contributors_stale_at = datetime($retry_at)",
                )
                .param("key", full_name.to_ascii_lowercase())
                .param("error", error.to_string())
                .param(
                    "retry_at",
                    (Utc::now() + Duration::minutes(INSIGHT_REFRESH_RETRY_MINUTES)).to_rfc3339(),
                ),
            )
            .await
    }

    async fn fail_user_commit_repository_refresh(&self, login: &str, error: &str) -> AppResult<()> {
        self.client
            .run(
                query(
                    "MATCH (user:User {login_key: $key})
                     SET user.commit_activity_refresh_started_at = null,
                         user.commit_activity_last_refresh_error = $error,
                         user.commit_activity_stale_at = datetime($retry_at)",
                )
                .param("key", login.to_ascii_lowercase())
                .param("error", error.to_string())
                .param(
                    "retry_at",
                    (Utc::now() + Duration::minutes(INSIGHT_REFRESH_RETRY_MINUTES)).to_rfc3339(),
                ),
            )
            .await
    }
}

impl Neo4jInsightRepository {
    async fn begin_refresh(&self, q: neo4rs::Query) -> AppResult<bool> {
        let mut rows = self
            .client
            .graph
            .execute_on(&self.client.database, q)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Ok(false);
        };
        Ok(row.get::<i64>("updated").map_err(map_neo4j_decode)? > 0)
    }
}

fn decode_insight_cache(row: &Row) -> AppResult<CacheMetadata> {
    Ok(CacheMetadata {
        last_fetched_at: optional_timestamp(row, "last_fetched_at")?,
        stale_at: optional_timestamp(row, "stale_at")?,
        refresh_started_at: optional_timestamp(row, "refresh_started_at")?,
        last_refresh_error: optional_string(row, "last_refresh_error")?,
    })
}

fn required_cache_timestamp(timestamp: Option<DateTime<Utc>>, field: &str) -> AppResult<String> {
    timestamp.map(|value| value.to_rfc3339()).ok_or_else(|| {
        AppError::Storage(format!(
            "insight cache is missing required `{field}` metadata"
        ))
    })
}

impl Neo4jDiscoveryRepository {
    async fn discovery_user(&self, login: &str) -> AppResult<(DiscoveryUser, GraphImportCoverage)> {
        let mut rows = self
            .client
            .graph
            .execute_on(
                &self.client.database,
                query(
                    "MATCH (user:User)
                     WHERE user.login_key = $login_key
                     RETURN user.github_id AS github_id,
                            user.login AS login,
                            user.name AS name,
                            user.url AS url,
                            user.avatar_url AS avatar_url,
                            user.bio AS bio,
                            user.followers_count AS followers_count,
                            user.following_count AS following_count,
                            user.public_repositories_count AS public_repositories_count,
                            toString(user.neighborhood_last_fetched_at) AS neighborhood_last_fetched_at,
                            toString(user.neighborhood_stale_at) AS neighborhood_stale_at,
                             coalesce(user.followers_complete, false) AS followers_complete,
                             coalesce(user.following_complete, false) AS following_complete,
                             coalesce(user.starred_repositories_complete, false) AS starred_repositories_complete,
                             coalesce(user.repositories_complete, false) AS repositories_complete
                     LIMIT 1",
                )
                .param("login_key", login.to_ascii_lowercase()),
            )
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        else {
            return Err(AppError::NotFound(format!(
                "github user `{login}` is not present in the shared graph"
            )));
        };
        let coverage = GraphImportCoverage {
            followers_complete: row
                .get::<bool>("followers_complete")
                .map_err(map_neo4j_decode)?,
            following_complete: row
                .get::<bool>("following_complete")
                .map_err(map_neo4j_decode)?,
            starred_repositories_complete: row
                .get::<bool>("starred_repositories_complete")
                .map_err(map_neo4j_decode)?,
            repositories_complete: row
                .get::<bool>("repositories_complete")
                .map_err(map_neo4j_decode)?,
        };
        let mut user = decode_discovery_user(&row)?;
        if !coverage.is_complete() {
            user.neighborhood_cache_status = CacheStatus::Stale;
        }
        Ok((user, coverage))
    }

    async fn related_users(&self, q: neo4rs::Query) -> AppResult<Vec<DiscoveryUser>> {
        let mut rows = self
            .client
            .graph
            .execute_on(&self.client.database, q)
            .await
            .map_err(|error| AppError::External(error.to_string()))?;
        let mut users = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| AppError::External(error.to_string()))?
        {
            users.push(decode_discovery_user(&row)?);
        }
        Ok(users)
    }
}

fn decode_discovery_user(row: &Row) -> AppResult<DiscoveryUser> {
    let neighborhood_last_fetched_at = row
        .get::<Option<String>>("neighborhood_last_fetched_at")
        .map_err(map_neo4j_decode)?
        .and_then(|value| parse_timestamp(&value));
    let neighborhood_stale_at = row
        .get::<Option<String>>("neighborhood_stale_at")
        .map_err(map_neo4j_decode)?
        .and_then(|value| parse_timestamp(&value));
    let (neighborhood_cache_status, _, _) = cache_status_from_metadata(CacheMetadata {
        last_fetched_at: neighborhood_last_fetched_at,
        stale_at: neighborhood_stale_at,
        refresh_started_at: None,
        last_refresh_error: None,
    });
    Ok(DiscoveryUser {
        profile: GitHubUserNode {
            github_id: row.get::<i64>("github_id").map_err(map_neo4j_decode)?,
            login: row.get::<String>("login").map_err(map_neo4j_decode)?,
            name: optional_string(row, "name")?,
            url: row.get::<String>("url").map_err(map_neo4j_decode)?,
            avatar_url: optional_string(row, "avatar_url")?,
            bio: optional_string(row, "bio")?,
            followers_count: optional_u64(row, "followers_count")?,
            following_count: optional_u64(row, "following_count")?,
            public_repositories_count: optional_u64(row, "public_repositories_count")?,
        },
        neighborhood_cache_status,
        neighborhood_last_fetched_at,
    })
}

fn decode_repository_node(row: &Row) -> AppResult<GitHubRepositoryNode> {
    Ok(GitHubRepositoryNode {
        github_id: row.get::<i64>("github_id").map_err(map_neo4j_decode)?,
        owner_login: row.get::<String>("owner_login").map_err(map_neo4j_decode)?,
        name: row.get::<String>("name").map_err(map_neo4j_decode)?,
        full_name: row.get::<String>("full_name").map_err(map_neo4j_decode)?,
        description: optional_string(row, "description")?,
        html_url: row.get::<String>("html_url").map_err(map_neo4j_decode)?,
        stargazer_count: optional_u64(row, "stargazer_count")?.unwrap_or_default(),
        fork_count: optional_u64(row, "fork_count")?.unwrap_or_default(),
        language: optional_string(row, "language")?,
        topics: row
            .get::<Option<Vec<String>>>("topics")
            .map_err(map_neo4j_decode)?
            .unwrap_or_default(),
        pushed_at: optional_timestamp(row, "pushed_at")?,
        updated_at: optional_timestamp(row, "updated_at")?,
        archived: row
            .get::<Option<bool>>("archived")
            .map_err(map_neo4j_decode)?
            .unwrap_or(false),
        is_fork: row
            .get::<Option<bool>>("is_fork")
            .map_err(map_neo4j_decode)?
            .unwrap_or(false),
    })
}

fn optional_string(row: &Row, key: &str) -> AppResult<Option<String>> {
    Ok(row
        .get::<Option<String>>(key)
        .map_err(map_neo4j_decode)?
        .filter(|value| !value.is_empty()))
}

fn optional_u64(row: &Row, key: &str) -> AppResult<Option<u64>> {
    Ok(row
        .get::<Option<i64>>(key)
        .map_err(map_neo4j_decode)?
        .and_then(|value| u64::try_from(value).ok()))
}

fn optional_timestamp(row: &Row, key: &str) -> AppResult<Option<DateTime<Utc>>> {
    Ok(row
        .get::<Option<String>>(key)
        .map_err(map_neo4j_decode)?
        .and_then(|value| parse_timestamp(&value)))
}

fn parse_required_timestamp(value: &str) -> AppResult<DateTime<Utc>> {
    parse_timestamp(value).ok_or_else(|| {
        AppError::External(format!(
            "failed to parse timestamp value `{value}` from Neo4j"
        ))
    })
}

fn seed_type(seed: &ExplorationSeed) -> &'static str {
    match seed {
        ExplorationSeed::User { .. } => "user",
        ExplorationSeed::Repository { .. } => "repository",
        ExplorationSeed::Category { .. } => "category",
    }
}

fn seed_value(seed: &ExplorationSeed) -> String {
    match seed {
        ExplorationSeed::User { login } => login.clone(),
        ExplorationSeed::Repository { full_name } => full_name.clone(),
        ExplorationSeed::Category { name } => name.clone(),
    }
}

fn parse_seed(kind: &str, value: &str) -> AppResult<ExplorationSeed> {
    match kind {
        "user" => Ok(ExplorationSeed::User {
            login: value.to_string(),
        }),
        "repository" => Ok(ExplorationSeed::Repository {
            full_name: value.to_string(),
        }),
        "category" => Ok(ExplorationSeed::Category {
            name: value.to_string(),
        }),
        other => Err(AppError::External(format!(
            "unknown exploration seed type `{other}` from Neo4j"
        ))),
    }
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut deduped = values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    deduped.sort();
    deduped.dedup();
    deduped
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubApiUser {
    id: i64,
    login: String,
    name: Option<String>,
    html_url: String,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    bio: Option<String>,
    #[serde(default)]
    followers: Option<u64>,
    #[serde(default)]
    following: Option<u64>,
    #[serde(default)]
    public_repos: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubApiContributor {
    id: i64,
    login: String,
    #[serde(default)]
    avatar_url: Option<String>,
    html_url: String,
    contributions: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubPublicEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "repo")]
    repository: GitHubEventRepository,
    #[serde(default)]
    payload: GitHubPushPayload,
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct GitHubEventRepository {
    id: i64,
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct GitHubPushPayload {
    #[serde(default)]
    size: u64,
}

fn aggregate_user_commit_events(
    events: Vec<GitHubPublicEvent>,
    source_complete: bool,
) -> UserCommitRepositoriesSnapshot {
    let source_event_count = events.len();
    let source_truncated =
        !source_complete || source_event_count == USER_COMMIT_ACTIVITY_EVENT_LIMIT;
    let mut repositories = HashMap::<String, UserCommitRepository>::new();
    for event in events {
        if event.kind != "PushEvent" {
            continue;
        }
        let Some(created_at) = event.created_at else {
            continue;
        };
        let commit_count = event.payload.size;
        repositories
            .entry(event.repository.name.clone())
            .and_modify(|repository| {
                repository.push_count = repository.push_count.saturating_add(1);
                repository.commit_count = repository.commit_count.saturating_add(commit_count);
                repository.last_pushed_at = repository.last_pushed_at.max(created_at);
            })
            .or_insert_with(|| UserCommitRepository {
                github_id: event.repository.id,
                full_name: event.repository.name.clone(),
                url: format!("https://github.com/{}", event.repository.name),
                push_count: 1,
                commit_count,
                last_pushed_at: created_at,
            });
    }
    let mut repositories = repositories.into_values().collect::<Vec<_>>();
    repositories.sort_by(|left, right| {
        right
            .commit_count
            .cmp(&left.commit_count)
            .then_with(|| right.push_count.cmp(&left.push_count))
            .then_with(|| right.last_pushed_at.cmp(&left.last_pushed_at))
            .then_with(|| left.full_name.cmp(&right.full_name))
    });
    UserCommitRepositoriesSnapshot {
        repositories,
        source_event_count,
        source_truncated,
    }
}

impl From<GitHubApiUser> for GitHubUserNode {
    fn from(value: GitHubApiUser) -> Self {
        Self {
            github_id: value.id,
            login: value.login,
            name: value.name,
            url: value.html_url,
            avatar_url: value.avatar_url,
            bio: value.bio,
            followers_count: value.followers,
            following_count: value.following,
            public_repositories_count: value.public_repos,
        }
    }
}

impl From<octocrab::models::Repository> for GitHubRepositoryNode {
    fn from(value: octocrab::models::Repository) -> Self {
        let owner_login = value
            .owner
            .as_ref()
            .map(|owner| owner.login.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let full_name = value
            .full_name
            .clone()
            .unwrap_or_else(|| format!("{owner_login}/{}", value.name));
        let language = value
            .language
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        Self {
            github_id: value.id.0 as i64,
            owner_login,
            name: value.name,
            full_name,
            description: value.description,
            html_url: value
                .html_url
                .map(|url| url.to_string())
                .unwrap_or_default(),
            stargazer_count: value.stargazers_count.unwrap_or_default() as u64,
            fork_count: value.forks_count.unwrap_or_default() as u64,
            language,
            topics: value.topics.unwrap_or_default(),
            pushed_at: value.pushed_at,
            updated_at: value.updated_at,
            archived: value.archived.unwrap_or(false),
            is_fork: value.fork.unwrap_or(false),
        }
    }
}

#[derive(Debug, Serialize)]
struct GitHubListParams {
    per_page: u8,
}

#[derive(Debug, Serialize)]
struct GitHubContributorParams {
    per_page: u8,
    anon: bool,
}

impl Default for GitHubContributorParams {
    fn default() -> Self {
        Self {
            per_page: REPOSITORY_CONTRIBUTOR_LIMIT as u8,
            anon: false,
        }
    }
}

impl Default for GitHubListParams {
    fn default() -> Self {
        Self { per_page: 100 }
    }
}

#[derive(Debug, Serialize)]
struct GitHubRepositoryListParams {
    per_page: u8,
    #[serde(rename = "type")]
    kind: &'static str,
    sort: &'static str,
    direction: &'static str,
}

impl Default for GitHubRepositoryListParams {
    fn default() -> Self {
        Self {
            per_page: 100,
            kind: "owner",
            sort: "updated",
            direction: "desc",
        }
    }
}

#[derive(Debug, Serialize)]
struct BrowserCodeExchangeRequest {
    client_id: String,
    client_secret: String,
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserOAuthResponse {
    access_token: String,
    scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredDeviceCodes {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

impl From<DeviceCodes> for StoredDeviceCodes {
    fn from(value: DeviceCodes) -> Self {
        Self {
            device_code: value.device_code,
            user_code: value.user_code,
            verification_uri: value.verification_uri,
            expires_in: value.expires_in,
            interval: value.interval,
        }
    }
}

#[derive(Debug, Serialize)]
struct DeviceAccessTokenRequest {
    client_id: String,
    device_code: String,
    grant_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_IDENTITY_KEY: &str = "HC7oBiY2DcgwFPFOplgK1nk77uOQV7x__mLWuIEBFz4";

    #[test]
    fn graph_capacity_estimate_deduplicates_incoming_entities_and_relationships() {
        let user = |github_id, login: &str| GitHubUserNode {
            github_id,
            login: login.to_string(),
            url: format!("https://github.com/{login}"),
            ..Default::default()
        };
        let repository = |github_id, full_name: &str| {
            let (owner_login, name) = full_name.split_once('/').expect("owner/name");
            GitHubRepositoryNode {
                github_id,
                owner_login: owner_login.to_string(),
                name: name.to_string(),
                full_name: full_name.to_string(),
                html_url: format!("https://github.com/{full_name}"),
                ..Default::default()
            }
        };
        let import = GraphImport {
            viewer: Some(user(1, "viewer")),
            followers: vec![user(2, "two"), user(2, "two"), user(3, "three")],
            following: vec![user(2, "two"), user(4, "four")],
            repositories: vec![
                repository(10, "viewer/ten"),
                repository(10, "viewer/ten"),
                repository(11, "viewer/eleven"),
            ],
            starred_repositories: vec![
                repository(10, "viewer/ten"),
                repository(12, "other/twelve"),
                repository(12, "other/twelve"),
            ],
            ..Default::default()
        };

        assert_eq!(
            GraphImportCapacityEstimate::from_import(&import),
            GraphImportCapacityEstimate {
                nodes: 7,
                relationships: 8,
            }
        );
    }

    #[test]
    fn graph_capacity_gate_allows_equality_and_rejects_each_exceeded_dimension() {
        let incoming = GraphImportCapacityEstimate {
            nodes: 7,
            relationships: 8,
        };
        enforce_graph_capacity(100, 200, incoming, Some(107), Some(208))
            .expect("configured maxima are inclusive");

        assert!(matches!(
            enforce_graph_capacity(100, 200, incoming, Some(106), Some(208)),
            Err(AppError::GraphCapacityExceeded {
                resource,
                current_count: 100,
                incoming_count: 7,
                projected_count: 107,
                maximum_count: 106,
            }) if resource == "nodes"
        ));
        assert!(matches!(
            enforce_graph_capacity(100, 200, incoming, None, Some(207)),
            Err(AppError::GraphCapacityExceeded {
                resource,
                current_count: 200,
                incoming_count: 8,
                projected_count: 208,
                maximum_count: 207,
            }) if resource == "relationships"
        ));
    }

    #[test]
    fn oauth_identity_lookup_preserves_the_minimum_rest_reserve() {
        let mut status = GitHubRateLimitStatus {
            limit: 5_000,
            used: 3_999,
            remaining: 1_001,
            reset_at: Utc::now() + Duration::minutes(30),
            checked_at: Utc::now(),
        };
        OctocrabGitHubClient::ensure_oauth_identity_budget_status(&status)
            .expect("1,001 requests admits the one-request identity lookup");

        status.remaining = 1_000;
        let error = OctocrabGitHubClient::ensure_oauth_identity_budget_status(&status)
            .expect_err("1,000 requests must preserve the reserve");
        assert!(matches!(
            error,
            AppError::RateBudgetReserved {
                operation,
                remaining: 1_000,
                reserve: 1_000,
                requested_cost: 1,
                ..
            } if operation == "github_oauth_identity"
        ));
    }

    #[test]
    fn identity_cipher_rejects_tampering_and_context_swaps() {
        let key = SecretString::from(TEST_IDENTITY_KEY.to_string());
        let cipher = IdentityCipher::from_secret(&key).expect("valid identity key");
        let ciphertext = cipher
            .encrypt(GITHUB_TOKEN_PURPOSE, "42", "github-token")
            .expect("encrypt token");

        assert_ne!(ciphertext, "github-token");
        assert_eq!(
            cipher
                .decrypt(GITHUB_TOKEN_PURPOSE, "42", &ciphertext)
                .expect("decrypt token"),
            "github-token"
        );
        assert!(
            cipher
                .decrypt(GITHUB_TOKEN_PURPOSE, "43", &ciphertext)
                .is_err(),
            "associated data must bind ciphertext to its GitHub account"
        );

        let mut tampered = ciphertext.into_bytes();
        let last = tampered.last_mut().expect("ciphertext byte");
        *last = if *last == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered).expect("ASCII ciphertext");
        assert!(
            cipher
                .decrypt(GITHUB_TOKEN_PURPOSE, "42", &tampered)
                .is_err(),
            "authenticated encryption must reject modified ciphertext"
        );
    }

    #[tokio::test]
    async fn json_identity_upgrade_encrypts_secrets_and_persists_pending_oauth() {
        let directory = tempfile::tempdir().expect("temp identity directory");
        let path = directory.path().join("identity.json");
        let now = Utc::now();
        let mut state = IdentityStore::default();
        state.connections.insert(
            "app-user".to_string(),
            GitHubConnection {
                account: ConnectedAccount {
                    github_user_id: 42,
                    login: "octocat".to_string(),
                    display_name: Some("Octocat".to_string()),
                },
                access_token: "plaintext-github-token".to_string(),
                scopes: vec!["read:user".to_string()],
            },
        );
        state.pending_browser_logins.insert(
            "oauth-state".to_string(),
            PendingBrowserLogin {
                user_id: "app-user".to_string(),
                redirect_to: Some("https://example.test/app".to_string()),
                browser_nonce: "plaintext-browser-nonce".to_string(),
                created_at: now,
                expires_at: now + Duration::minutes(10),
            },
        );
        state.create_session("browser-session", "app-user");
        write_json_file(&path, &state).expect("write legacy identity");

        let key = SecretString::from(TEST_IDENTITY_KEY.to_string());
        let first =
            JsonIdentityRepository::new(path.clone(), &key).expect("upgrade identity store");
        let serialized = std::fs::read_to_string(&path).expect("read upgraded identity");
        assert!(!serialized.contains("plaintext-github-token"));
        assert!(!serialized.contains("plaintext-browser-nonce"));
        assert!(serialized.contains("v1."));
        assert_eq!(
            first
                .get_connection("app-user")
                .await
                .expect("read connection")
                .expect("stored connection")
                .access_token,
            "plaintext-github-token"
        );
        drop(first);

        let second =
            JsonIdentityRepository::new(path, &key).expect("reopen encrypted identity store");
        assert_eq!(
            second
                .consume_pending_browser_login("oauth-state")
                .await
                .expect("consume pending state")
                .expect("pending state")
                .browser_nonce,
            "plaintext-browser-nonce"
        );
        assert_eq!(
            second
                .get_user_id_for_session("browser-session")
                .await
                .expect("resolve session")
                .as_deref(),
            Some("app-user")
        );
    }

    #[tokio::test]
    async fn json_identity_persists_rate_status_and_fenced_lease() {
        let directory = tempfile::tempdir().expect("temp identity directory");
        let path = directory.path().join("identity.json");
        let key = SecretString::from(TEST_IDENTITY_KEY.to_string());
        let status = GitHubRateLimitStatus {
            limit: 5_000,
            used: 3_750,
            remaining: 1_250,
            reset_at: Utc::now() + Duration::minutes(45),
            checked_at: Utc::now(),
        };
        let first = JsonIdentityRepository::new(path.clone(), &key).expect("identity store");
        first
            .save_github_rate_limit(42, status.clone())
            .await
            .expect("persist rate status");
        let lease = first
            .try_acquire_github_rate_limit_lease(42, "first-owner", 120)
            .await
            .expect("acquire persisted lease")
            .expect("lease owner");
        drop(first);

        let second = JsonIdentityRepository::new(path, &key).expect("reopen identity store");
        assert_eq!(
            second
                .github_rate_limit(42)
                .await
                .expect("read persisted rate status"),
            Some(status)
        );
        assert!(
            second
                .try_acquire_github_rate_limit_lease(42, "contender", 120)
                .await
                .expect("contend for persisted lease")
                .is_none()
        );
        assert!(
            second
                .release_github_rate_limit_lease(&lease)
                .await
                .expect("release persisted lease")
        );
        assert!(
            second
                .try_acquire_github_rate_limit_lease(42, "replacement", 120)
                .await
                .expect("replacement lease")
                .is_some()
        );
    }

    #[tokio::test]
    async fn legacy_graph_without_coverage_loads_as_incomplete_and_stale() {
        let mut serialized = serde_json::to_value(GraphStore::default()).expect("serialize graph");
        serialized
            .get_mut("shared")
            .and_then(serde_json::Value::as_object_mut)
            .expect("shared graph object")
            .remove("user_coverage");
        let mut graph: GraphStore =
            serde_json::from_value(serialized).expect("deserialize legacy graph");
        graph.shared.users.insert(
            "alice".to_string(),
            GitHubUserNode {
                github_id: 1,
                login: "alice".to_string(),
                url: "https://github.com/alice".to_string(),
                ..Default::default()
            },
        );
        graph.shared.user_cache.insert(
            "alice".to_string(),
            CacheMetadata {
                last_fetched_at: Some(Utc::now()),
                stale_at: Some(Utc::now() + Duration::hours(6)),
                ..Default::default()
            },
        );
        let repository = LocalDiscoveryRepository {
            store: Arc::new(LocalGraphStore {
                path: None,
                state: Mutex::new(graph),
            }),
        };

        let neighborhood = repository
            .user_neighborhood("app-user", "alice")
            .await
            .expect("legacy neighborhood");

        assert_eq!(neighborhood.coverage, GraphImportCoverage::incomplete());
        assert_eq!(
            neighborhood.user.neighborhood_cache_status,
            CacheStatus::Stale
        );
    }

    #[test]
    fn session_normalization_and_creation_enforce_the_global_bound() {
        let mut store = IdentityStore::default();
        for index in 0..(MAX_ACTIVE_SESSIONS + 128) {
            store.sessions.insert(
                format!("session-{index}"),
                StoredSession::Current(SessionRecord {
                    user_id: "app-user".to_string(),
                    expires_at: Utc::now() + Duration::days(SESSION_TTL_DAYS),
                }),
            );
        }

        assert!(store.normalize_legacy_data());
        assert_eq!(store.sessions.len(), MAX_ACTIVE_SESSIONS);

        store.create_session("replacement-session", "app-user");
        assert_eq!(store.sessions.len(), MAX_ACTIVE_SESSIONS);
        assert!(store.sessions.contains_key("replacement-session"));
    }

    #[test]
    fn case_only_alias_changes_relocate_stable_local_nodes_and_bookmarks() {
        let mut store = GraphStore::default();
        let fetched_at = Utc::now();
        upsert_import(
            &mut store,
            GraphImport {
                viewer: Some(GitHubUserNode {
                    github_id: 1,
                    login: "Alice".to_string(),
                    url: "https://github.com/Alice".to_string(),
                    ..Default::default()
                }),
                followers: vec![GitHubUserNode {
                    github_id: 2,
                    login: "Bob".to_string(),
                    url: "https://github.com/Bob".to_string(),
                    ..Default::default()
                }],
                repositories: vec![GitHubRepositoryNode {
                    github_id: 10,
                    owner_login: "Alice".to_string(),
                    name: "Tool".to_string(),
                    full_name: "Alice/Tool".to_string(),
                    html_url: "https://github.com/Alice/Tool".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            fetched_at,
            fetched_at + Duration::hours(6),
        );
        store.bookmarks.insert(
            "app-user".to_string(),
            vec![Bookmark {
                id: "bookmark".to_string(),
                target: BookmarkTarget::GitHubRepository {
                    full_name: "Alice/Tool".to_string(),
                },
                categories: Vec::new(),
                note: None,
                created_at: fetched_at,
            }],
        );

        upsert_import(
            &mut store,
            GraphImport {
                viewer: Some(GitHubUserNode {
                    github_id: 1,
                    login: "alice".to_string(),
                    url: "https://github.com/alice".to_string(),
                    ..Default::default()
                }),
                followers: vec![GitHubUserNode {
                    github_id: 2,
                    login: "bob".to_string(),
                    url: "https://github.com/bob".to_string(),
                    ..Default::default()
                }],
                repositories: vec![GitHubRepositoryNode {
                    github_id: 10,
                    owner_login: "alice".to_string(),
                    name: "tool".to_string(),
                    full_name: "alice/tool".to_string(),
                    html_url: "https://github.com/alice/tool".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            fetched_at,
            fetched_at + Duration::hours(6),
        );

        assert_eq!(
            store
                .shared
                .users
                .values()
                .filter(|user| user.github_id == 1)
                .count(),
            1
        );
        assert!(store.shared.users.contains_key("alice"));
        assert!(!store.shared.users.contains_key("Alice"));
        assert_eq!(
            store
                .shared
                .repositories
                .values()
                .filter(|repository| repository.github_id == 10)
                .count(),
            1
        );
        assert!(store.shared.repositories.contains_key("alice/tool"));
        assert_eq!(
            store.bookmarks["app-user"][0].target,
            BookmarkTarget::GitHubRepository {
                full_name: "alice/tool".to_string()
            }
        );
    }

    #[test]
    fn public_push_events_are_aggregated_by_repository_and_commit_count() {
        let now = Utc::now();
        let snapshot = aggregate_user_commit_events(
            vec![
                GitHubPublicEvent {
                    kind: "PushEvent".to_string(),
                    repository: GitHubEventRepository {
                        id: 10,
                        name: "acme/tool".to_string(),
                    },
                    payload: GitHubPushPayload { size: 2 },
                    created_at: Some(now - Duration::hours(2)),
                },
                GitHubPublicEvent {
                    kind: "WatchEvent".to_string(),
                    repository: GitHubEventRepository {
                        id: 11,
                        name: "acme/ignored".to_string(),
                    },
                    payload: GitHubPushPayload::default(),
                    created_at: Some(now),
                },
                GitHubPublicEvent {
                    kind: "PushEvent".to_string(),
                    repository: GitHubEventRepository {
                        id: 10,
                        name: "acme/tool".to_string(),
                    },
                    payload: GitHubPushPayload { size: 3 },
                    created_at: Some(now),
                },
            ],
            true,
        );

        assert_eq!(snapshot.source_event_count, 3);
        assert!(!snapshot.source_truncated);
        assert_eq!(snapshot.repositories.len(), 1);
        assert_eq!(snapshot.repositories[0].push_count, 2);
        assert_eq!(snapshot.repositories[0].commit_count, 5);
        assert_eq!(snapshot.repositories[0].last_pushed_at, now);
    }

    #[tokio::test]
    #[ignore = "requires a live Neo4j database configured through GITEXPLORE_NEO4J_* variables"]
    async fn live_neo4j_refresh_lease_is_exclusive_and_fenced() {
        let config = Neo4jConfig {
            uri: Some(std::env::var("GITEXPLORE_NEO4J_URI").expect("Neo4j URI")),
            username: Some(std::env::var("GITEXPLORE_NEO4J_USERNAME").expect("Neo4j username")),
            password: Some(SecretString::from(
                std::env::var("GITEXPLORE_NEO4J_PASSWORD").expect("Neo4j password"),
            )),
            database: std::env::var("GITEXPLORE_NEO4J_DATABASE")
                .unwrap_or_else(|_| "neo4j".to_string()),
            max_total_nodes: None,
            max_total_relationships: None,
        };
        let client = Arc::new(Neo4jClient::new(&config).await.expect("Neo4j client"));
        let repository = Neo4jGitHubImportRepository {
            client: client.clone(),
            max_total_nodes: None,
            max_total_relationships: None,
        };
        let entity_key = format!("test-refresh-lease:{}", Uuid::new_v4());
        let first = match repository
            .try_acquire_refresh_lease(&entity_key, "first", 120)
            .await
            .expect("first lease")
        {
            RefreshLeaseAttempt::Acquired(lease) => lease,
            RefreshLeaseAttempt::Busy(_) => panic!("first lease was busy"),
        };
        assert!(matches!(
            repository
                .try_acquire_refresh_lease(&entity_key, "contender", 120)
                .await
                .expect("contender"),
            RefreshLeaseAttempt::Busy(_)
        ));
        assert!(
            repository
                .complete_refresh_lease(&first, Some("{}"))
                .await
                .expect("complete first")
        );
        let replacement = match repository
            .try_acquire_refresh_lease(&entity_key, "replacement", 120)
            .await
            .expect("replacement")
        {
            RefreshLeaseAttempt::Acquired(lease) => lease,
            RefreshLeaseAttempt::Busy(_) => panic!("replacement was busy"),
        };
        assert!(
            !repository
                .fail_refresh_lease(&first, "stale owner")
                .await
                .expect("stale failure")
        );
        assert!(
            repository
                .fail_refresh_lease(&replacement, "test cleanup")
                .await
                .expect("replacement cleanup")
        );
        client
            .run(
                query("MATCH (lease:RefreshLease {entity_key: $entity_key}) DELETE lease")
                    .param("entity_key", entity_key),
            )
            .await
            .expect("delete test lease");
    }
}
