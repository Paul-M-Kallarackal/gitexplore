use std::{
    collections::HashMap,
    env,
    io::Write,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use secrecy::{ExposeSecret, SecretString};

use crate::shared::{AppError, AppResult, ensure};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server_addr: String,
    pub frontend_origin: String,
    pub data_dir: PathBuf,
    pub deployment_mode: DeploymentMode,
    pub graph_backend: GraphBackend,
    pub identity_encryption_key: Option<SecretString>,
    pub github: GitHubConfig,
    pub neo4j: Neo4jConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphBackend {
    Memory,
    File,
    Neo4j,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    Local,
    Production,
}

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub client_id: Option<SecretString>,
    pub client_secret: Option<SecretString>,
    pub scopes: Vec<String>,
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Neo4jConfig {
    pub uri: Option<String>,
    pub username: Option<String>,
    pub password: Option<SecretString>,
    pub database: String,
    pub max_total_nodes: Option<usize>,
    pub max_total_relationships: Option<usize>,
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        let map = env::vars().collect::<HashMap<_, _>>();
        Self::from_map(&map)
    }

    pub fn from_map(values: &HashMap<String, String>) -> AppResult<Self> {
        let server_addr = match (values.get("GITEXPLORE_SERVER_ADDR"), values.get("PORT")) {
            (Some(address), _) => address.clone(),
            (None, Some(port)) => {
                let port = port.parse::<u16>().map_err(|_| {
                    AppError::Config("PORT must be an integer between 1 and 65535".to_string())
                })?;
                ensure(port > 0, "PORT must be an integer between 1 and 65535")?;
                format!("0.0.0.0:{port}")
            }
            (None, None) => "127.0.0.1:4000".to_string(),
        };
        let data_dir = PathBuf::from(
            values
                .get("GITEXPLORE_DATA_DIR")
                .cloned()
                .unwrap_or_else(|| ".gitexplore-data".to_string()),
        );
        let graph_backend = match values
            .get("GITEXPLORE_GRAPH_BACKEND")
            .map(String::as_str)
            .unwrap_or("file")
        {
            "memory" => GraphBackend::Memory,
            "file" => GraphBackend::File,
            "neo4j" => GraphBackend::Neo4j,
            other => {
                return Err(AppError::Config(format!(
                    "invalid graph backend `{other}`; expected memory|file|neo4j"
                )));
            }
        };
        let deployment_mode = match values
            .get("GITEXPLORE_DEPLOYMENT_MODE")
            .map(String::as_str)
            .unwrap_or("local")
        {
            "local" => DeploymentMode::Local,
            "production" => DeploymentMode::Production,
            other => {
                return Err(AppError::Config(format!(
                    "invalid deployment mode `{other}`; expected local|production"
                )));
            }
        };

        let github = GitHubConfig {
            client_id: values
                .get("GITEXPLORE_GITHUB_CLIENT_ID")
                .cloned()
                .map(SecretString::from),
            client_secret: values
                .get("GITEXPLORE_GITHUB_CLIENT_SECRET")
                .cloned()
                .map(SecretString::from),
            scopes: values
                .get("GITEXPLORE_GITHUB_SCOPES")
                .map(|scopes| {
                    scopes
                        .split(',')
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_else(|| vec!["read:user".to_string()]),
            redirect_uri: values.get("GITEXPLORE_GITHUB_REDIRECT_URI").cloned(),
        };

        let neo4j = Neo4jConfig {
            uri: values.get("GITEXPLORE_NEO4J_URI").cloned(),
            username: values.get("GITEXPLORE_NEO4J_USERNAME").cloned(),
            password: values
                .get("GITEXPLORE_NEO4J_PASSWORD")
                .cloned()
                .map(SecretString::from),
            database: values
                .get("GITEXPLORE_NEO4J_DATABASE")
                .cloned()
                .unwrap_or_else(|| "neo4j".to_string()),
            max_total_nodes: optional_positive_usize(values, "GITEXPLORE_NEO4J_MAX_TOTAL_NODES")?,
            max_total_relationships: optional_positive_usize(
                values,
                "GITEXPLORE_NEO4J_MAX_TOTAL_RELATIONSHIPS",
            )?,
        };

        let config = Self {
            server_addr,
            frontend_origin: values
                .get("GITEXPLORE_FRONTEND_ORIGIN")
                .cloned()
                .unwrap_or_else(|| "http://localhost:3000".to_string()),
            data_dir,
            deployment_mode,
            graph_backend,
            identity_encryption_key: values
                .get("GITEXPLORE_IDENTITY_ENCRYPTION_KEY")
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .map(SecretString::from),
            github,
            neo4j,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> AppResult<()> {
        let frontend_origin = url::Url::parse(&self.frontend_origin)
            .map_err(|error| AppError::Config(format!("invalid frontend origin: {error}")))?;
        ensure(
            matches!(frontend_origin.scheme(), "http" | "https")
                && frontend_origin.host_str().is_some()
                && frontend_origin.username().is_empty()
                && frontend_origin.password().is_none()
                && frontend_origin.path() == "/"
                && frontend_origin.query().is_none()
                && frontend_origin.fragment().is_none(),
            "GITEXPLORE_FRONTEND_ORIGIN must be an http(s) origin without a path, query, or fragment",
        )?;
        if self.graph_backend == GraphBackend::Neo4j {
            ensure(
                self.neo4j.uri.is_some(),
                "neo4j backend requires GITEXPLORE_NEO4J_URI",
            )?;
            ensure(
                self.neo4j.username.is_some(),
                "neo4j backend requires GITEXPLORE_NEO4J_USERNAME",
            )?;
            ensure(
                self.neo4j.password.is_some(),
                "neo4j backend requires GITEXPLORE_NEO4J_PASSWORD",
            )?;
        }
        if self.graph_backend != GraphBackend::Memory && self.identity_encryption_key.is_none() {
            return Err(AppError::Config(
                "durable identity storage requires GITEXPLORE_IDENTITY_ENCRYPTION_KEY".to_string(),
            ));
        }
        if let Some(encoded_key) = &self.identity_encryption_key {
            let decoded = URL_SAFE_NO_PAD
                .decode(encoded_key.expose_secret())
                .map_err(|_| {
                    AppError::Config(
                        "GITEXPLORE_IDENTITY_ENCRYPTION_KEY must be unpadded base64url encoding of exactly 32 random bytes"
                            .to_string(),
                    )
                })?;
            if decoded.len() != 32 {
                return Err(AppError::Config(
                    "GITEXPLORE_IDENTITY_ENCRYPTION_KEY must be unpadded base64url encoding of exactly 32 random bytes"
                        .to_string(),
                ));
            }
        }
        if self.deployment_mode == DeploymentMode::Production {
            ensure(
                self.graph_backend == GraphBackend::Neo4j,
                "production requires GITEXPLORE_GRAPH_BACKEND=neo4j",
            )?;
            ensure(
                frontend_origin.scheme() == "https",
                "production requires an HTTPS GITEXPLORE_FRONTEND_ORIGIN",
            )?;
            ensure(
                self.github
                    .client_id
                    .as_ref()
                    .is_some_and(|value| !value.expose_secret().trim().is_empty()),
                "production requires GITEXPLORE_GITHUB_CLIENT_ID",
            )?;
            ensure(
                self.github
                    .client_secret
                    .as_ref()
                    .is_some_and(|value| !value.expose_secret().trim().is_empty()),
                "production requires GITEXPLORE_GITHUB_CLIENT_SECRET",
            )?;
            let expected_redirect = frontend_origin
                .join("/auth/oauth/callback")
                .expect("validated frontend origin joins a fixed callback path");
            ensure(
                self.github.redirect_uri.as_deref() == Some(expected_redirect.as_str()),
                "production GitHub redirect URI must exactly match <frontend-origin>/auth/oauth/callback",
            )?;
            ensure(
                self.neo4j.uri.as_deref().is_some_and(|uri| {
                    uri.starts_with("neo4j+s://") || uri.starts_with("bolt+s://")
                }),
                "production requires an encrypted Neo4j URI (neo4j+s:// or bolt+s://)",
            )?;
            ensure(
                self.neo4j
                    .username
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
                "production requires GITEXPLORE_NEO4J_USERNAME",
            )?;
            ensure(
                self.neo4j
                    .password
                    .as_ref()
                    .is_some_and(|value| !value.expose_secret().trim().is_empty()),
                "production requires GITEXPLORE_NEO4J_PASSWORD",
            )?;
            ensure(
                !self.neo4j.database.trim().is_empty(),
                "production requires GITEXPLORE_NEO4J_DATABASE",
            )?;
            ensure(
                self.neo4j
                    .max_total_nodes
                    .is_some_and(|limit| limit <= 190_000),
                "production requires GITEXPLORE_NEO4J_MAX_TOTAL_NODES at or below 190000",
            )?;
            ensure(
                self.neo4j
                    .max_total_relationships
                    .is_some_and(|limit| limit <= 380_000),
                "production requires GITEXPLORE_NEO4J_MAX_TOTAL_RELATIONSHIPS at or below 380000",
            )?;
        }
        Ok(())
    }

    pub fn identity_store_path(&self) -> PathBuf {
        self.data_dir.join("identity.json")
    }

    pub fn graph_store_path(&self) -> PathBuf {
        self.data_dir.join("graph.json")
    }

    pub fn ensure_data_dir(&self) -> AppResult<()> {
        if !self.data_dir.exists() {
            std::fs::create_dir_all(&self.data_dir)?;
        }
        Ok(())
    }
}

fn optional_positive_usize(
    values: &HashMap<String, String>,
    key: &str,
) -> AppResult<Option<usize>> {
    values
        .get(key)
        .map(|value| {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| AppError::Config(format!("{key} must be a positive integer")))?;
            ensure(parsed > 0, format!("{key} must be a positive integer"))?;
            Ok(parsed)
        })
        .transpose()
}

pub fn write_json_file<T: serde::Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let serialized = serde_json::to_vec_pretty(value)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.as_file_mut().write_all(&serialized)?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| AppError::Io(error.error))?;
    Ok(())
}

pub fn read_json_file<T: serde::de::DeserializeOwned + Default>(path: &Path) -> AppResult<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::AppConfig;

    #[test]
    fn vercel_port_binds_the_server_to_all_interfaces() {
        let values = HashMap::from([
            ("PORT".to_string(), "8080".to_string()),
            ("GITEXPLORE_GRAPH_BACKEND".to_string(), "memory".to_string()),
        ]);

        let config = AppConfig::from_map(&values).expect("PORT should produce a valid address");

        assert_eq!(config.server_addr, "0.0.0.0:8080");
    }

    #[test]
    fn explicit_server_address_takes_precedence_over_port() {
        let values = HashMap::from([
            (
                "GITEXPLORE_SERVER_ADDR".to_string(),
                "127.0.0.1:4100".to_string(),
            ),
            ("PORT".to_string(), "8080".to_string()),
            ("GITEXPLORE_GRAPH_BACKEND".to_string(), "memory".to_string()),
        ]);

        let config = AppConfig::from_map(&values).expect("explicit address should be valid");

        assert_eq!(config.server_addr, "127.0.0.1:4100");
    }

    #[test]
    fn invalid_vercel_port_is_rejected() {
        let values = HashMap::from([("PORT".to_string(), "not-a-port".to_string())]);

        let error = AppConfig::from_map(&values).expect_err("invalid PORT must fail");

        assert_eq!(
            error.to_string(),
            "configuration error: PORT must be an integer between 1 and 65535"
        );
    }

    #[test]
    fn neo4j_identity_fails_closed_without_an_encryption_key() {
        let values = HashMap::from([
            ("GITEXPLORE_GRAPH_BACKEND".to_string(), "neo4j".to_string()),
            (
                "GITEXPLORE_NEO4J_URI".to_string(),
                "neo4j://localhost:7687".to_string(),
            ),
            ("GITEXPLORE_NEO4J_USERNAME".to_string(), "neo4j".to_string()),
            (
                "GITEXPLORE_NEO4J_PASSWORD".to_string(),
                "password".to_string(),
            ),
        ]);

        let error = AppConfig::from_map(&values).expect_err("missing identity key must fail");

        assert!(
            error
                .to_string()
                .contains("GITEXPLORE_IDENTITY_ENCRYPTION_KEY")
        );
    }

    #[test]
    fn file_identity_fails_closed_without_an_encryption_key() {
        let values = HashMap::from([("GITEXPLORE_GRAPH_BACKEND".to_string(), "file".to_string())]);

        let error = AppConfig::from_map(&values).expect_err("missing identity key must fail");

        assert!(
            error
                .to_string()
                .contains("GITEXPLORE_IDENTITY_ENCRYPTION_KEY")
        );
    }

    #[test]
    fn identity_encryption_key_requires_exact_base64url_length() {
        let values = HashMap::from([
            (
                "GITEXPLORE_IDENTITY_ENCRYPTION_KEY".to_string(),
                "not-a-32-byte-key".to_string(),
            ),
            ("GITEXPLORE_GRAPH_BACKEND".to_string(), "memory".to_string()),
        ]);

        let error = AppConfig::from_map(&values).expect_err("invalid identity key must fail");

        assert!(error.to_string().contains("unpadded base64url"));
    }

    #[test]
    fn production_mode_rejects_ephemeral_storage_defaults() {
        let values = HashMap::from([
            (
                "GITEXPLORE_DEPLOYMENT_MODE".to_string(),
                "production".to_string(),
            ),
            (
                "GITEXPLORE_IDENTITY_ENCRYPTION_KEY".to_string(),
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
            ),
        ]);

        let error = AppConfig::from_map(&values).expect_err("production file mode must fail");

        assert!(error.to_string().contains("GRAPH_BACKEND=neo4j"));
    }

    #[test]
    fn production_mode_accepts_the_complete_durable_contract() {
        let values = HashMap::from([
            (
                "GITEXPLORE_DEPLOYMENT_MODE".to_string(),
                "production".to_string(),
            ),
            ("GITEXPLORE_GRAPH_BACKEND".to_string(), "neo4j".to_string()),
            (
                "GITEXPLORE_IDENTITY_ENCRYPTION_KEY".to_string(),
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
            ),
            (
                "GITEXPLORE_FRONTEND_ORIGIN".to_string(),
                "https://gitexplore.example.com".to_string(),
            ),
            (
                "GITEXPLORE_GITHUB_CLIENT_ID".to_string(),
                "client".to_string(),
            ),
            (
                "GITEXPLORE_GITHUB_CLIENT_SECRET".to_string(),
                "secret".to_string(),
            ),
            (
                "GITEXPLORE_GITHUB_REDIRECT_URI".to_string(),
                "https://gitexplore.example.com/auth/oauth/callback".to_string(),
            ),
            (
                "GITEXPLORE_NEO4J_URI".to_string(),
                "neo4j+s://example.databases.neo4j.io".to_string(),
            ),
            ("GITEXPLORE_NEO4J_USERNAME".to_string(), "neo4j".to_string()),
            (
                "GITEXPLORE_NEO4J_PASSWORD".to_string(),
                "password".to_string(),
            ),
            (
                "GITEXPLORE_NEO4J_MAX_TOTAL_NODES".to_string(),
                "190000".to_string(),
            ),
            (
                "GITEXPLORE_NEO4J_MAX_TOTAL_RELATIONSHIPS".to_string(),
                "380000".to_string(),
            ),
        ]);

        let config = AppConfig::from_map(&values).expect("production contract should validate");

        assert_eq!(config.neo4j.max_total_nodes, Some(190_000));
        assert_eq!(config.neo4j.max_total_relationships, Some(380_000));
    }

    #[test]
    fn invalid_neo4j_capacity_limit_is_rejected() {
        let values = HashMap::from([
            ("GITEXPLORE_GRAPH_BACKEND".to_string(), "memory".to_string()),
            (
                "GITEXPLORE_NEO4J_MAX_TOTAL_NODES".to_string(),
                "0".to_string(),
            ),
        ]);

        let error = AppConfig::from_map(&values).expect_err("zero capacity must fail");

        assert!(error.to_string().contains("MAX_TOTAL_NODES"));
    }
}
