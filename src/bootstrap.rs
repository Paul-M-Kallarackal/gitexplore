use std::sync::Arc;

use crate::{
    adapters::{LocalRepositorySet, Neo4jRepositorySet, OctocrabGitHubClient},
    application::{AppServiceRepositories, AppServices},
    config::{AppConfig, GraphBackend},
    ports::GitHubAuthConfig,
    shared::AppResult,
};

#[derive(Clone)]
pub struct AppState {
    pub services: AppServices,
    pub frontend_origin: String,
    pub graph_backend: GraphBackend,
}

pub async fn build_app_state(config: AppConfig) -> AppResult<AppState> {
    config.ensure_data_dir()?;

    let github = Arc::new(OctocrabGitHubClient::new()?);
    let github_auth = GitHubAuthConfig {
        client_id: config
            .github
            .client_id
            .clone()
            .unwrap_or_else(|| secrecy::SecretString::from(String::new())),
        client_secret: config.github.client_secret.clone(),
        redirect_uri: config.github.redirect_uri.clone(),
        scopes: config.github.scopes.clone(),
    };

    let services = match config.graph_backend {
        GraphBackend::Memory => {
            let repositories = LocalRepositorySet::in_memory();
            AppServices::new(
                AppServiceRepositories {
                    identity: repositories.identity,
                    imports: repositories.imports,
                    sync_state: repositories.sync_state,
                    categories: repositories.categories,
                    bookmarks: repositories.bookmarks,
                    exploration: repositories.exploration,
                    discovery: repositories.discovery,
                    insights: repositories.insights,
                },
                github,
                github_auth,
            )
        }
        GraphBackend::File => {
            let identity_encryption_key =
                config.identity_encryption_key.as_ref().ok_or_else(|| {
                    crate::shared::AppError::Config(
                        "file backend requires GITEXPLORE_IDENTITY_ENCRYPTION_KEY".to_string(),
                    )
                })?;
            let repositories = LocalRepositorySet::from_files(
                config.identity_store_path(),
                config.graph_store_path(),
                identity_encryption_key,
            )?;
            AppServices::new(
                AppServiceRepositories {
                    identity: repositories.identity,
                    imports: repositories.imports,
                    sync_state: repositories.sync_state,
                    categories: repositories.categories,
                    bookmarks: repositories.bookmarks,
                    exploration: repositories.exploration,
                    discovery: repositories.discovery,
                    insights: repositories.insights,
                },
                github,
                github_auth,
            )
        }
        GraphBackend::Neo4j => {
            let identity_encryption_key =
                config.identity_encryption_key.as_ref().ok_or_else(|| {
                    crate::shared::AppError::Config(
                        "neo4j backend requires GITEXPLORE_IDENTITY_ENCRYPTION_KEY".to_string(),
                    )
                })?;
            let repositories =
                Neo4jRepositorySet::new(&config.neo4j, identity_encryption_key).await?;
            AppServices::new(
                AppServiceRepositories {
                    identity: repositories.identity,
                    imports: repositories.imports,
                    sync_state: repositories.sync_state,
                    categories: repositories.categories,
                    bookmarks: repositories.bookmarks,
                    exploration: repositories.exploration,
                    discovery: repositories.discovery,
                    insights: repositories.insights,
                },
                github,
                github_auth,
            )
        }
    };

    Ok(AppState {
        services,
        frontend_origin: config.frontend_origin,
        graph_backend: config.graph_backend,
    })
}
