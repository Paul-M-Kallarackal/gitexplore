pub mod adapters;
pub mod application;
pub mod bookmarks;
pub mod bootstrap;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod exploration;
pub mod graph;
pub mod graphql;
pub mod http;
pub mod identity;
pub mod insights;
pub mod ports;
pub mod schema;
pub mod shared;

pub use cli::run_cli;

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use crate::{
        adapters::{LocalRepositorySet, StubGitHubClient},
        application::{AppServiceRepositories, AppServices},
        bookmarks::{Bookmark, BookmarkTarget, Category},
        bootstrap::AppState,
        config::AppConfig,
        discovery::RepositoryReasonKind,
        exploration::ExplorationSeed,
        graph::{
            CacheStatus, GitHubRateLimitLease, GitHubRateLimitStatus, GitHubRepositoryNode,
            GitHubUserNode, GraphImport, GraphImportCoverage, RefreshLease, RefreshLeaseAttempt,
            RefreshLeaseState, RefreshLeaseStatus, SyncState, SyncSummary, UserRefreshOutcome,
        },
        identity::{ConnectedAccount, GitHubConnection},
        insights::{
            RepositoryContributor, RepositoryContributorInsights, RepositoryContributorsSnapshot,
            UserCommitRepositoriesSnapshot, UserCommitRepository,
        },
        ports::{DeviceLoginStart, GitHubAuthConfig, GitHubClientPort, GitHubImportRepository},
        shared::{AppError, AppResult},
    };

    #[test]
    fn config_requires_github_client_id_for_device_auth() {
        let mut env = HashMap::new();
        env.insert("GITEXPLORE_DATA_DIR".to_string(), "data".to_string());
        env.insert(
            "GITEXPLORE_SERVER_ADDR".to_string(),
            "127.0.0.1:4000".to_string(),
        );
        env.insert("GITEXPLORE_GRAPH_BACKEND".to_string(), "memory".to_string());

        let config = AppConfig::from_map(&env).expect("config should parse defaults");
        assert_eq!(config.server_addr, "127.0.0.1:4000");
        assert!(config.github.client_id.is_none());
    }

    #[tokio::test]
    async fn app_state_exposes_expected_services() {
        let repositories = LocalRepositorySet::in_memory();
        let github = Arc::new(StubGitHubClient::default());

        let services = AppServices::new(
            AppServiceRepositories {
                identity: repositories.identity.clone(),
                imports: repositories.imports.clone(),
                sync_state: repositories.sync_state.clone(),
                categories: repositories.categories.clone(),
                bookmarks: repositories.bookmarks.clone(),
                exploration: repositories.exploration.clone(),
                discovery: repositories.discovery.clone(),
                insights: repositories.insights.clone(),
            },
            github.clone(),
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );
        let state = AppState {
            services,
            frontend_origin: "http://localhost:3000".to_string(),
            graph_backend: crate::config::GraphBackend::Memory,
        };

        let status = state
            .services
            .identity
            .connection_status("default")
            .await
            .expect("identity status");

        assert!(!status.connected);
    }

    #[tokio::test]
    async fn browser_oauth_state_is_opaque_single_use_and_session_bound() {
        let repositories = LocalRepositorySet::in_memory();
        let services = AppServices::new(
            AppServiceRepositories {
                identity: repositories.identity.clone(),
                imports: repositories.imports,
                sync_state: repositories.sync_state,
                categories: repositories.categories,
                bookmarks: repositories.bookmarks,
                exploration: repositories.exploration,
                discovery: repositories.discovery,
                insights: repositories.insights,
            },
            Arc::new(StubGitHubClient::default()),
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );

        let oauth_url = services
            .identity
            .start_browser_login(
                "browser-user",
                Some("http://localhost:3000/app/explore".to_string()),
                "browser-nonce",
            )
            .await
            .expect("start browser login");
        let state_id = url::Url::parse(&oauth_url)
            .expect("oauth url")
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .expect("opaque state");

        let completed = services
            .identity
            .complete_browser_login(&state_id, "stub-code", "browser-nonce")
            .await
            .expect("complete browser login");
        assert_eq!(
            completed.redirect_to.as_deref(),
            Some("http://localhost:3000/app/explore")
        );
        assert_eq!(
            services
                .identity
                .resolve_session(&completed.session_id)
                .await
                .expect("resolve browser session")
                .as_deref(),
            Some("browser-user")
        );

        services
            .identity
            .logout("browser-user")
            .await
            .expect("disconnect first browser connection");
        let second_oauth_url = services
            .identity
            .start_browser_login("new-browser-user", None, "second-browser-nonce")
            .await
            .expect("start second browser login");
        let second_state_id = url::Url::parse(&second_oauth_url)
            .expect("second oauth url")
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .expect("second opaque state");
        let second_completed = services
            .identity
            .complete_browser_login(&second_state_id, "stub-code", "second-browser-nonce")
            .await
            .expect("complete second browser login");
        assert_eq!(
            services
                .identity
                .resolve_session(&second_completed.session_id)
                .await
                .expect("resolve second browser session")
                .as_deref(),
            Some("browser-user"),
            "the same GitHub account should reuse its private overlay"
        );

        let replay = services
            .identity
            .complete_browser_login(&state_id, "stub-code", "browser-nonce")
            .await;
        assert!(matches!(replay, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn browser_oauth_can_start_and_complete_on_different_service_replicas() {
        let repositories = LocalRepositorySet::in_memory();
        let github = Arc::new(StubGitHubClient::default());
        let auth = GitHubAuthConfig {
            client_id: secrecy::SecretString::from("stub-client"),
            client_secret: None,
            redirect_uri: None,
            scopes: vec!["read:user".to_string()],
        };
        let first = AppServices::new(
            AppServiceRepositories {
                identity: repositories.identity.clone(),
                imports: repositories.imports.clone(),
                sync_state: repositories.sync_state.clone(),
                categories: repositories.categories.clone(),
                bookmarks: repositories.bookmarks.clone(),
                exploration: repositories.exploration.clone(),
                discovery: repositories.discovery.clone(),
                insights: repositories.insights.clone(),
            },
            github.clone(),
            auth.clone(),
        );
        let second = AppServices::new(
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
            auth,
        );

        let oauth_url = first
            .identity
            .start_browser_login("cross-replica-user", None, "cross-replica-nonce")
            .await
            .expect("start OAuth on first replica");
        let state_id = url::Url::parse(&oauth_url)
            .expect("OAuth URL")
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .expect("OAuth state");
        let completed = second
            .identity
            .complete_browser_login(&state_id, "stub-code", "cross-replica-nonce")
            .await
            .expect("complete OAuth on second replica");

        assert_eq!(
            first
                .identity
                .resolve_session(&completed.session_id)
                .await
                .expect("resolve session on first replica")
                .as_deref(),
            Some("cross-replica-user")
        );
    }

    #[tokio::test]
    async fn concurrent_repository_saves_are_idempotent() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .imports
            .import_github_graph(
                "default",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    repositories: vec![test_repository(2, "labs/rare-tool", "Rust", 50, 4)],
                    ..Default::default()
                },
            )
            .await
            .expect("seed repository graph");
        let services = AppServices::new(
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
            Arc::new(StubGitHubClient::default()),
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );
        let target = BookmarkTarget::GitHubRepository {
            full_name: "labs/rare-tool".to_string(),
        };
        let first_service = services.bookmarks.clone();
        let second_service = services.bookmarks.clone();
        let first_target = target.clone();
        let (first, second) = tokio::join!(
            first_service.add_bookmark("default", first_target, vec!["rare".to_string()], None),
            second_service.add_bookmark("default", target, Vec::new(), None)
        );
        let first = first.expect("first concurrent save");
        let second = second.expect("second concurrent save");
        assert_eq!(first.id, second.id);
        assert_eq!(
            services
                .bookmarks
                .list_bookmarks("default")
                .await
                .expect("list concurrent saves")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn local_repository_set_shares_imports_and_isolates_private_data() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .imports
            .import_github_graph(
                "alice-user",
                GraphImport {
                    viewer: Some(GitHubUserNode {
                        github_id: 1,
                        login: "alice".to_string(),
                        name: Some("Alice".to_string()),
                        url: "https://github.com/alice".to_string(),
                        ..Default::default()
                    }),
                    followers: vec![GitHubUserNode {
                        github_id: 2,
                        login: "bob".to_string(),
                        name: None,
                        url: "https://github.com/bob".to_string(),
                        ..Default::default()
                    }],
                    following: vec![GitHubUserNode {
                        github_id: 3,
                        login: "carol".to_string(),
                        name: None,
                        url: "https://github.com/carol".to_string(),
                        ..Default::default()
                    }],
                    starred_repositories: vec![GitHubRepositoryNode {
                        github_id: 10,
                        owner_login: "acme".to_string(),
                        name: "tool".to_string(),
                        full_name: "acme/tool".to_string(),
                        description: None,
                        html_url: "https://github.com/acme/tool".to_string(),
                        ..Default::default()
                    }],
                    repositories: vec![GitHubRepositoryNode {
                        github_id: 11,
                        owner_login: "alice".to_string(),
                        name: "gitexplore".to_string(),
                        full_name: "alice/gitexplore".to_string(),
                        description: None,
                        html_url: "https://github.com/alice/gitexplore".to_string(),
                        ..Default::default()
                    }],
                    coverage: GraphImportCoverage::default(),
                },
            )
            .await
            .expect("graph import should persist");

        repositories
            .categories
            .create_category(
                "alice-user",
                Category {
                    name: "people".to_string(),
                    description: None,
                },
            )
            .await
            .expect("category create");

        repositories
            .imports
            .resolve_bookmark_target(&BookmarkTarget::GitHubUser {
                login: "bob".to_string(),
            })
            .await
            .expect("shared import repo should resolve bookmark target for another user");

        repositories
            .bookmarks
            .add_bookmark(
                "alice-user",
                crate::bookmarks::Bookmark {
                    id: "bookmark-1".to_string(),
                    target: BookmarkTarget::GitHubUser {
                        login: "bob".to_string(),
                    },
                    categories: vec!["people".to_string()],
                    note: None,
                    created_at: chrono::Utc::now(),
                },
            )
            .await
            .expect("bookmark create");

        let exploration = repositories
            .exploration
            .explore(
                "alice-user",
                ExplorationSeed::Category {
                    name: "people".to_string(),
                },
            )
            .await
            .expect("exploration query");

        assert!(exploration.related_people.contains(&"bob".to_string()));
        assert_eq!(exploration.cache_status.to_string(), "fresh");

        let other_user_bookmarks = repositories
            .bookmarks
            .list_bookmarks("bob-user")
            .await
            .expect("other user bookmarks");
        assert!(other_user_bookmarks.is_empty());
    }

    #[tokio::test]
    async fn sync_service_uses_separate_import_and_sync_repositories() {
        let repositories = LocalRepositorySet::in_memory();
        let github = Arc::new(StubGitHubClient {
            import: GraphImport {
                viewer: Some(GitHubUserNode {
                    github_id: 1,
                    login: "stub-user".to_string(),
                    name: Some("Stub User".to_string()),
                    url: "https://github.com/stub-user".to_string(),
                    ..Default::default()
                }),
                followers: Vec::new(),
                following: Vec::new(),
                starred_repositories: Vec::new(),
                repositories: Vec::new(),
                coverage: GraphImportCoverage::default(),
            },
        });

        repositories
            .identity
            .save_connection(
                "default",
                GitHubConnection {
                    account: crate::identity::ConnectedAccount {
                        github_user_id: 1,
                        login: "stub-user".to_string(),
                        display_name: Some("Stub User".to_string()),
                    },
                    access_token: "token".to_string(),
                    scopes: vec!["read:user".to_string()],
                },
            )
            .await
            .expect("connection save");

        let services = AppServices::new(
            AppServiceRepositories {
                identity: repositories.identity.clone(),
                imports: repositories.imports.clone(),
                sync_state: repositories.sync_state.clone(),
                categories: repositories.categories.clone(),
                bookmarks: repositories.bookmarks.clone(),
                exploration: repositories.exploration.clone(),
                discovery: repositories.discovery.clone(),
                insights: repositories.insights.clone(),
            },
            github,
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );

        let summary = services.sync.run_sync("default").await.expect("sync run");
        let rate_limit = services
            .sync
            .rate_limit("default")
            .await
            .expect("rate limit");
        let status = repositories
            .sync_state
            .sync_status("default")
            .await
            .expect("sync status");

        assert_eq!(summary.repositories, 0);
        assert_eq!(rate_limit.limit, 5_000);
        assert_eq!(rate_limit.remaining, 5_000);
        assert!(matches!(status.state, SyncState::SyncSucceeded));
    }

    #[tokio::test]
    async fn http_routes_use_session_cookie_for_private_data() {
        let state = seeded_http_state().await;
        let app = crate::http::router(Arc::new(state));

        let auth_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/auth/status")
                    .header("cookie", "gitexplore_session=test-session")
                    .body(Body::empty())
                    .expect("auth status request"),
            )
            .await
            .expect("auth status response");
        assert_eq!(auth_response.status(), StatusCode::OK);
        let auth_body = to_bytes(auth_response.into_body(), usize::MAX)
            .await
            .expect("auth status body");
        let auth_body: serde_json::Value =
            serde_json::from_slice(&auth_body).expect("auth status json");
        assert_eq!(auth_body["app_user_id"], "default");

        let create_category = Request::builder()
            .method("POST")
            .uri("/categories")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "name": "people",
                    "description": "People to watch"
                })
                .to_string(),
            ))
            .expect("category request");
        let category_response = app
            .clone()
            .oneshot(create_category)
            .await
            .expect("category response");
        assert_eq!(category_response.status(), StatusCode::OK);

        let list_categories = Request::builder()
            .uri("/categories")
            .header("cookie", "gitexplore_session=test-session")
            .body(Body::empty())
            .expect("list categories request");
        let categories_response = app
            .clone()
            .oneshot(list_categories)
            .await
            .expect("list categories response");
        assert_eq!(categories_response.status(), StatusCode::OK);
        let categories_body = to_bytes(categories_response.into_body(), usize::MAX)
            .await
            .expect("categories body");
        assert!(String::from_utf8_lossy(&categories_body).contains("\"people\""));

        let add_bookmark = Request::builder()
            .method("POST")
            .uri("/bookmarks")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "target": { "GitHubUser": { "login": "bob" } },
                    "categories": ["people"],
                    "note": "follow up"
                })
                .to_string(),
            ))
            .expect("bookmark request");
        let bookmark_response = app
            .clone()
            .oneshot(add_bookmark)
            .await
            .expect("bookmark response");
        assert_eq!(bookmark_response.status(), StatusCode::OK);

        let list_bookmarks = Request::builder()
            .uri("/bookmarks")
            .header("cookie", "gitexplore_session=test-session")
            .body(Body::empty())
            .expect("list bookmarks request");
        let bookmarks_response = app
            .clone()
            .oneshot(list_bookmarks)
            .await
            .expect("list bookmarks response");
        assert_eq!(bookmarks_response.status(), StatusCode::OK);
        let bookmarks_body = to_bytes(bookmarks_response.into_body(), usize::MAX)
            .await
            .expect("bookmarks body");
        assert!(String::from_utf8_lossy(&bookmarks_body).contains("\"bob\""));

        let explore = Request::builder()
            .method("POST")
            .uri("/explore")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"seed_type":"category","seed_value":"people"}"#,
            ))
            .expect("explore request");
        let explore_response = app
            .clone()
            .oneshot(explore)
            .await
            .expect("explore response");
        assert_eq!(explore_response.status(), StatusCode::OK);
        let explore_body = to_bytes(explore_response.into_body(), usize::MAX)
            .await
            .expect("explore body");
        assert!(String::from_utf8_lossy(&explore_body).contains("\"related_people\""));

        let snapshots = Request::builder()
            .uri("/explore/snapshots")
            .header("cookie", "gitexplore_session=test-session")
            .body(Body::empty())
            .expect("snapshots request");
        let snapshots_response = app
            .clone()
            .oneshot(snapshots)
            .await
            .expect("snapshots response");
        assert_eq!(snapshots_response.status(), StatusCode::OK);
        let snapshots_body = to_bytes(snapshots_response.into_body(), usize::MAX)
            .await
            .expect("snapshots body");
        assert!(String::from_utf8_lossy(&snapshots_body).contains("\"seed\""));

        let graph_query = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "{ neighborhood(login: \"ALICE\", limit: 24) { user { login } followers { login } following { login } repositories { repository { fullName } } } }"
                })
                .to_string(),
            ))
            .expect("graphql neighborhood request");
        let graph_response = app
            .clone()
            .oneshot(graph_query)
            .await
            .expect("graphql neighborhood response");
        assert_eq!(graph_response.status(), StatusCode::OK);
        let graph_body = to_bytes(graph_response.into_body(), usize::MAX)
            .await
            .expect("graphql neighborhood body");
        let graph_body = String::from_utf8_lossy(&graph_body);
        assert!(graph_body.contains("\"followers\":[{\"login\":\"bob\"}]"));
        assert!(graph_body.contains("\"following\":[{\"login\":\"carol\"}]"));
        assert!(graph_body.contains("\"fullName\":\"acme/tool\""));

        let insight_query = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "{ rateLimit { limit remaining checkedAt } repositoryInsights(fullName: \"acme/tool\", limit: 12) { fullName sourceComplete cacheStatus } userInsights(login: \"alice\", limit: 12) { login windowDays sourceEventCount cacheStatus } }"
                })
                .to_string(),
            ))
            .expect("graphql insight request");
        let insight_response = app
            .clone()
            .oneshot(insight_query)
            .await
            .expect("graphql insight response");
        assert_eq!(insight_response.status(), StatusCode::OK);
        let insight_body = to_bytes(insight_response.into_body(), usize::MAX)
            .await
            .expect("graphql insight body");
        let insight_body = String::from_utf8_lossy(&insight_body);
        assert!(
            insight_body.contains("\"remaining\":5000"),
            "unexpected insight response: {insight_body}"
        );
        assert!(insight_body.contains("\"fullName\":\"acme/tool\""));
        assert!(insight_body.contains("\"windowDays\":30"));

        let simple_csrf_request = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "text/plain")
            .body(Body::from(
                r#"{"query":"mutation { saveRepository(fullName: \"acme/tool\", categories: [], note: null) { id } }"}"#,
            ))
            .expect("simple csrf request");
        let simple_csrf_response = app
            .clone()
            .oneshot(simple_csrf_request)
            .await
            .expect("simple csrf response");
        assert_eq!(
            simple_csrf_response.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );

        let hostile_origin_request = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .header("origin", "https://attacker.example")
            .body(Body::from(
                serde_json::json!({
                    "query": "mutation { saveRepository(fullName: \"acme/tool\", categories: [], note: null) { id } }"
                })
                .to_string(),
            ))
            .expect("hostile origin request");
        let hostile_origin_response = app
            .clone()
            .oneshot(hostile_origin_request)
            .await
            .expect("hostile origin response");
        assert_eq!(hostile_origin_response.status(), StatusCode::FORBIDDEN);

        let hostile_sync_request = Request::builder()
            .method("POST")
            .uri("/sync/run")
            .header("cookie", "gitexplore_session=test-session")
            .header("origin", "https://attacker.example")
            .body(Body::empty())
            .expect("hostile sync request");
        let hostile_sync_response = app
            .clone()
            .oneshot(hostile_sync_request)
            .await
            .expect("hostile sync response");
        assert_eq!(hostile_sync_response.status(), StatusCode::FORBIDDEN);

        let hostile_explore_request = Request::builder()
            .method("POST")
            .uri("/explore")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .header("origin", "https://attacker.example")
            .body(Body::from(
                r#"{"seed_type":"category","seed_value":"people"}"#,
            ))
            .expect("hostile explore request");
        let hostile_explore_response = app
            .clone()
            .oneshot(hostile_explore_request)
            .await
            .expect("hostile explore response");
        assert_eq!(hostile_explore_response.status(), StatusCode::FORBIDDEN);

        let save_repository = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "mutation { saveRepository(fullName: \"acme/tool\", categories: [\"rare\"], note: \"Worth revisiting\") { fullName categories note } }"
                })
                .to_string(),
            ))
            .expect("graphql save repository request");
        let save_response = app
            .clone()
            .oneshot(save_repository)
            .await
            .expect("graphql save repository response");
        assert_eq!(save_response.status(), StatusCode::OK);
        let save_body = to_bytes(save_response.into_body(), usize::MAX)
            .await
            .expect("graphql save repository body");
        let save_body = String::from_utf8_lossy(&save_body);
        assert!(save_body.contains("\"fullName\":\"acme/tool\""));
        assert!(save_body.contains("\"categories\":[\"rare\"]"));

        let duplicate_save_repository = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("cookie", "gitexplore_session=test-session")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "mutation { saveRepository(fullName: \"acme/tool\", categories: [], note: null) { id } }"
                })
                .to_string(),
            ))
            .expect("duplicate graphql save request");
        let duplicate_save_response = app
            .clone()
            .oneshot(duplicate_save_repository)
            .await
            .expect("duplicate graphql save response");
        assert_eq!(duplicate_save_response.status(), StatusCode::OK);

        let saved_repositories = Request::builder()
            .uri("/bookmarks")
            .header("cookie", "gitexplore_session=test-session")
            .body(Body::empty())
            .expect("saved repositories request");
        let saved_repositories_response = app
            .clone()
            .oneshot(saved_repositories)
            .await
            .expect("saved repositories response");
        let saved_repositories_body = to_bytes(saved_repositories_response.into_body(), usize::MAX)
            .await
            .expect("saved repositories body");
        let saved_repositories: Vec<crate::bookmarks::Bookmark> =
            serde_json::from_slice(&saved_repositories_body).expect("saved repositories json");
        assert_eq!(
            saved_repositories
                .iter()
                .filter(|bookmark| {
                    bookmark.target
                        == BookmarkTarget::GitHubRepository {
                            full_name: "acme/tool".to_string(),
                        }
                })
                .count(),
            1
        );

        let logout = Request::builder()
            .method("POST")
            .uri("/auth/logout")
            .header("cookie", "gitexplore_session=test-session")
            .header("origin", "http://localhost:3000")
            .body(Body::empty())
            .expect("logout request");
        let logout_response = app.clone().oneshot(logout).await.expect("logout response");
        assert_eq!(logout_response.status(), StatusCode::OK);
        assert!(
            logout_response
                .headers()
                .get("set-cookie")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("Max-Age=0"))
        );
        let cleared_session = Request::builder()
            .uri("/bookmarks")
            .header("cookie", "gitexplore_session=test-session")
            .body(Body::empty())
            .expect("cleared-session request");
        let cleared_session_response = app
            .clone()
            .oneshot(cleared_session)
            .await
            .expect("cleared-session response");
        assert_eq!(cleared_session_response.status(), StatusCode::UNAUTHORIZED);

        let unauthorized = Request::builder()
            .uri("/bookmarks")
            .body(Body::empty())
            .expect("unauthorized bookmarks request");
        let unauthorized_response = app
            .clone()
            .oneshot(unauthorized)
            .await
            .expect("unauthorized bookmarks response");
        assert_eq!(unauthorized_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn local_discovery_preserves_direction_ranks_candidates_and_marks_saved_repositories() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .imports
            .import_github_graph(
                "alice-app",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    followers: vec![test_user(2, "bob")],
                    following: vec![test_user(3, "carol")],
                    starred_repositories: vec![test_repository(
                        10,
                        "seed/rust-tool",
                        "Rust",
                        2_000,
                        120,
                    )],
                    repositories: Vec::new(),
                    coverage: GraphImportCoverage::default(),
                },
            )
            .await
            .expect("alice graph import");

        let initial = repositories
            .discovery
            .user_neighborhood("alice-app", "alice")
            .await
            .expect("alice neighborhood");
        assert_eq!(
            initial
                .followers
                .iter()
                .map(|user| user.profile.login.as_str())
                .collect::<Vec<_>>(),
            vec!["bob"]
        );
        assert_eq!(
            initial
                .following
                .iter()
                .map(|user| user.profile.login.as_str())
                .collect::<Vec<_>>(),
            vec!["carol"]
        );
        assert_eq!(
            initial.followers[0].neighborhood_cache_status,
            CacheStatus::Stale
        );
        assert!(initial.followers[0].neighborhood_last_fetched_at.is_none());

        let hidden = test_repository(20, "labs/hidden-rust", "Rust", 800, 90);
        repositories
            .imports
            .import_github_graph(
                "alice-app",
                GraphImport {
                    viewer: Some(test_user(2, "bob")),
                    followers: Vec::new(),
                    following: vec![test_user(1, "alice")],
                    starred_repositories: vec![hidden.clone()],
                    repositories: Vec::new(),
                    coverage: GraphImportCoverage::default(),
                },
            )
            .await
            .expect("bob expansion");
        repositories
            .imports
            .import_github_graph(
                "alice-app",
                GraphImport {
                    viewer: Some(test_user(3, "carol")),
                    followers: vec![test_user(1, "alice")],
                    following: Vec::new(),
                    starred_repositories: Vec::new(),
                    repositories: vec![hidden.clone()],
                    coverage: GraphImportCoverage::default(),
                },
            )
            .await
            .expect("carol expansion");
        repositories
            .bookmarks
            .add_bookmark(
                "alice-app",
                Bookmark {
                    id: "saved-hidden".to_string(),
                    target: BookmarkTarget::GitHubRepository {
                        full_name: hidden.full_name.clone(),
                    },
                    categories: vec!["hidden-gems".to_string()],
                    note: Some("promising".to_string()),
                    created_at: chrono::Utc::now(),
                },
            )
            .await
            .expect("save hidden repository");

        let candidates = repositories
            .discovery
            .discover_repositories("alice-app", "alice", 10)
            .await
            .expect("rank repository candidates");
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(
            candidate.repository.repository.full_name,
            "labs/hidden-rust"
        );
        assert!(candidate.repository.saved);
        assert_eq!(candidate.via_logins, vec!["bob", "carol"]);
        assert_eq!(candidate.graph_signals.recommenders, 2);
        assert_eq!(candidate.graph_signals.followed_recommenders, 1);
        assert_eq!(candidate.graph_signals.follower_recommenders, 1);
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| { reason.kind == RepositoryReasonKind::LanguageMatch })
        );
        assert!(
            candidate
                .reasons
                .iter()
                .any(|reason| { reason.kind == RepositoryReasonKind::HiddenGem })
        );

        let other_user_candidates = repositories
            .discovery
            .discover_repositories("other-app", "alice", 10)
            .await
            .expect("other user's discovery");
        assert!(!other_user_candidates[0].repository.saved);
    }

    #[tokio::test]
    async fn partial_import_preserves_prior_edges_and_reports_incomplete_coverage() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .imports
            .import_github_graph(
                "alice-app",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    followers: vec![test_user(2, "bob")],
                    following: vec![test_user(3, "carol")],
                    ..Default::default()
                },
            )
            .await
            .expect("complete graph import");

        repositories
            .imports
            .import_github_graph(
                "alice-app",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    followers: vec![test_user(4, "dora")],
                    coverage: GraphImportCoverage {
                        followers_complete: false,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await
            .expect("partial graph import");

        let neighborhood = repositories
            .discovery
            .user_neighborhood("alice-app", "alice")
            .await
            .expect("partial neighborhood");
        let follower_logins = neighborhood
            .followers
            .iter()
            .map(|user| user.profile.login.as_str())
            .collect::<Vec<_>>();

        assert_eq!(follower_logins, vec!["bob", "dora"]);
        assert!(neighborhood.following.is_empty());
        assert!(!neighborhood.coverage.followers_complete);
        assert_eq!(
            neighborhood.user.neighborhood_cache_status,
            CacheStatus::Stale
        );
    }

    #[tokio::test]
    async fn local_repository_alias_changes_follow_stable_ids_and_saved_state() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .imports
            .import_github_graph(
                "alice-app",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    repositories: vec![test_repository(20, "labs/original-name", "Rust", 12, 1)],
                    ..Default::default()
                },
            )
            .await
            .expect("initial alias import");
        repositories
            .bookmarks
            .add_bookmark(
                "alice-app",
                Bookmark {
                    id: "stable-repository-save".to_string(),
                    target: BookmarkTarget::GitHubRepository {
                        full_name: "labs/original-name".to_string(),
                    },
                    categories: Vec::new(),
                    note: None,
                    created_at: chrono::Utc::now(),
                },
            )
            .await
            .expect("save original alias");

        repositories
            .imports
            .import_github_graph(
                "alice-app",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    repositories: vec![test_repository(20, "labs/renamed-project", "Rust", 13, 1)],
                    ..Default::default()
                },
            )
            .await
            .expect("renamed alias import");

        let bookmarks = repositories
            .bookmarks
            .list_bookmarks("alice-app")
            .await
            .expect("renamed bookmark");
        assert_eq!(
            bookmarks[0].target,
            BookmarkTarget::GitHubRepository {
                full_name: "labs/renamed-project".to_string(),
            }
        );
        let neighborhood = repositories
            .discovery
            .user_neighborhood("alice-app", "alice")
            .await
            .expect("renamed neighborhood");
        assert_eq!(
            neighborhood.owned_repositories[0].repository.full_name,
            "labs/renamed-project"
        );
    }

    #[tokio::test]
    async fn expand_user_deduplicates_concurrent_refreshes_but_allows_sequential_refreshes() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .identity
            .save_connection(
                "default",
                GitHubConnection {
                    account: ConnectedAccount {
                        github_user_id: 99,
                        login: "signed-in".to_string(),
                        display_name: None,
                    },
                    access_token: "token".to_string(),
                    scopes: vec!["read:user".to_string()],
                },
            )
            .await
            .expect("save test connection");
        let github = Arc::new(CountingGitHubClient {
            import: GraphImport {
                viewer: Some(test_user(7, "target")),
                followers: vec![test_user(8, "follower")],
                following: vec![test_user(9, "following")],
                starred_repositories: Vec::new(),
                repositories: Vec::new(),
                coverage: GraphImportCoverage::default(),
            },
            user_graph_calls: AtomicUsize::new(0),
        });
        let services = AppServices::new(
            AppServiceRepositories {
                identity: repositories.identity.clone(),
                imports: repositories.imports.clone(),
                sync_state: repositories.sync_state.clone(),
                categories: repositories.categories.clone(),
                bookmarks: repositories.bookmarks.clone(),
                exploration: repositories.exploration.clone(),
                discovery: repositories.discovery.clone(),
                insights: repositories.insights.clone(),
            },
            github.clone(),
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );

        let first = services.discovery.clone();
        let second = services.discovery.clone();
        let (first_result, second_result) = tokio::join!(
            first.expand_user("default", "target"),
            second.expand_user("default", "target")
        );
        assert_eq!(
            first_result.expect("first expansion").followers[0]
                .profile
                .login,
            "follower"
        );
        assert_eq!(
            second_result.expect("deduplicated expansion").following[0]
                .profile
                .login,
            "following"
        );
        assert_eq!(github.user_graph_calls.load(Ordering::SeqCst), 1);

        services
            .discovery
            .expand_user("default", "target")
            .await
            .expect("sequential explicit expansion");
        assert_eq!(github.user_graph_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn separate_app_instances_share_the_durable_refresh_lease() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .identity
            .save_connection(
                "default",
                GitHubConnection {
                    account: ConnectedAccount {
                        github_user_id: 99,
                        login: "signed-in".to_string(),
                        display_name: None,
                    },
                    access_token: "token".to_string(),
                    scopes: vec!["read:user".to_string()],
                },
            )
            .await
            .expect("save test connection");
        let durable_imports = Arc::new(SharedLeaseImportRepository::new(
            repositories.imports.clone(),
        ));
        let github = Arc::new(CountingGitHubClient {
            import: GraphImport {
                viewer: Some(test_user(7, "target")),
                followers: vec![test_user(8, "follower")],
                following: vec![test_user(9, "following")],
                ..Default::default()
            },
            user_graph_calls: AtomicUsize::new(0),
        });
        let make_services = || {
            AppServices::new(
                AppServiceRepositories {
                    identity: repositories.identity.clone(),
                    imports: durable_imports.clone(),
                    sync_state: repositories.sync_state.clone(),
                    categories: repositories.categories.clone(),
                    bookmarks: repositories.bookmarks.clone(),
                    exploration: repositories.exploration.clone(),
                    discovery: repositories.discovery.clone(),
                    insights: repositories.insights.clone(),
                },
                github.clone(),
                GitHubAuthConfig {
                    client_id: secrecy::SecretString::from("stub-client"),
                    client_secret: None,
                    redirect_uri: None,
                    scopes: vec!["read:user".to_string()],
                },
            )
        };
        let first = make_services();
        let second = make_services();

        let (first_result, second_result) = tokio::join!(
            first.discovery.expand_user("default", "target"),
            second.discovery.expand_user("default", "target")
        );

        assert_eq!(first_result.expect("first").followers.len(), 1);
        assert_eq!(second_result.expect("second").following.len(), 1);
        assert_eq!(github.user_graph_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_refresh_tokens_cannot_finish_a_replacement_lease() {
        let repositories = LocalRepositorySet::in_memory();
        let imports = SharedLeaseImportRepository::new(repositories.imports);
        let first = match imports
            .try_acquire_refresh_lease("github-user:target", "first", 120)
            .await
            .expect("first lease")
        {
            RefreshLeaseAttempt::Acquired(lease) => lease,
            RefreshLeaseAttempt::Busy(_) => panic!("first lease unexpectedly busy"),
        };
        assert!(
            imports
                .complete_refresh_lease(&first, Some("{}"))
                .await
                .expect("complete first")
        );
        let second = match imports
            .try_acquire_refresh_lease("github-user:target", "second", 120)
            .await
            .expect("second lease")
        {
            RefreshLeaseAttempt::Acquired(lease) => lease,
            RefreshLeaseAttempt::Busy(_) => panic!("replacement lease unexpectedly busy"),
        };

        assert!(
            !imports
                .fail_refresh_lease(&first, "stale failure")
                .await
                .expect("stale failure")
        );
        assert!(
            imports
                .renew_refresh_lease(&second, 120)
                .await
                .expect("renew replacement")
        );
    }

    #[tokio::test]
    async fn authenticated_rate_and_public_insights_are_cached() {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .identity
            .save_connection(
                "default",
                GitHubConnection {
                    account: ConnectedAccount {
                        github_user_id: 1,
                        login: "alice".to_string(),
                        display_name: Some("Alice".to_string()),
                    },
                    access_token: "token".to_string(),
                    scopes: vec!["read:user".to_string()],
                },
            )
            .await
            .expect("connection save");
        repositories
            .imports
            .import_github_graph(
                "default",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    repositories: vec![test_repository(2, "acme/tool", "Rust", 120, 8)],
                    ..Default::default()
                },
            )
            .await
            .expect("seed graph");
        let github = Arc::new(InsightGitHubClient::default());
        let services = AppServices::new(
            AppServiceRepositories {
                identity: repositories.identity.clone(),
                imports: repositories.imports.clone(),
                sync_state: repositories.sync_state.clone(),
                categories: repositories.categories.clone(),
                bookmarks: repositories.bookmarks.clone(),
                exploration: repositories.exploration.clone(),
                discovery: repositories.discovery.clone(),
                insights: repositories.insights.clone(),
            },
            github.clone(),
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );

        let first_rate = services.sync.rate_limit("default").await.expect("rate");
        let second_rate = services
            .sync
            .rate_limit("default")
            .await
            .expect("cached rate");
        assert_eq!(first_rate.checked_at, second_rate.checked_at);
        assert_eq!(github.rate_limit_calls.load(Ordering::SeqCst), 1);

        let first_insights = services.insights.clone();
        let second_insights = services.insights.clone();
        let (contributors, cached_contributors) = tokio::join!(
            first_insights.repository_contributors("default", "acme/tool", 12),
            second_insights.repository_contributors("default", "ACME/TOOL", 12)
        );
        let contributors = contributors.expect("contributors");
        let cached_contributors = cached_contributors.expect("deduplicated contributors");
        assert_eq!(contributors.contributors[0].login, "octocat");
        assert_eq!(cached_contributors.contributors[0].contributions, 42);
        assert_eq!(github.contributor_calls.load(Ordering::SeqCst), 1);

        let first_insights = services.insights.clone();
        let second_insights = services.insights.clone();
        let (activity, cached_activity) = tokio::join!(
            first_insights.user_commit_repositories("default", "alice", 12),
            second_insights.user_commit_repositories("default", "Alice", 12)
        );
        let activity = activity.expect("activity");
        let cached_activity = cached_activity.expect("deduplicated activity");
        assert_eq!(activity.repositories[0].full_name, "acme/tool");
        assert_eq!(cached_activity.repositories[0].commit_count, 7);
        assert_eq!(github.activity_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn github_rate_budget_lease_is_exclusive_and_fenced_per_identity() {
        let repositories = LocalRepositorySet::in_memory();
        let first_identity = repositories.identity.clone();
        let second_identity = repositories.identity.clone();
        let (first, second) = tokio::join!(
            first_identity.try_acquire_github_rate_limit_lease(99, "first", 120),
            second_identity.try_acquire_github_rate_limit_lease(99, "second", 120),
        );
        let first = first.expect("first contender");
        let second = second.expect("second contender");
        assert_ne!(first.is_some(), second.is_some());
        let lease = first.or(second).expect("one rate-budget lease owner");

        assert!(
            repositories
                .identity
                .renew_github_rate_limit_lease(&lease, 120)
                .await
                .expect("renew current owner")
        );
        let stale = GitHubRateLimitLease {
            github_user_id: lease.github_user_id,
            token: "stale-owner".to_string(),
            expires_at: lease.expires_at,
        };
        assert!(
            !repositories
                .identity
                .release_github_rate_limit_lease(&stale)
                .await
                .expect("reject stale owner")
        );
        assert!(
            repositories
                .identity
                .release_github_rate_limit_lease(&lease)
                .await
                .expect("release current owner")
        );
        assert!(
            repositories
                .identity
                .try_acquire_github_rate_limit_lease(99, "replacement", 120)
                .await
                .expect("replacement contender")
                .is_some()
        );
    }

    #[tokio::test]
    async fn github_rate_budget_enforces_exact_operation_boundaries() {
        let (services, github) = budget_test_services(1_013).await;

        services
            .discovery
            .expand_user("default", "allowed-expansion")
            .await
            .expect("1,013 remaining admits a 13-request expansion");
        assert_eq!(github.graph_calls.load(Ordering::SeqCst), 1);

        github.remaining.store(1_012, Ordering::SeqCst);
        assert_rate_budget_error(
            services
                .discovery
                .expand_user("default", "denied-expansion")
                .await
                .expect_err("1,012 remaining must preserve the reserve"),
            1_012,
            13,
            1_000,
        );
        assert_eq!(github.graph_calls.load(Ordering::SeqCst), 1);

        github.remaining.store(1_512, Ordering::SeqCst);
        assert_rate_budget_error(
            services
                .discovery
                .expand_user_with_reserve("default", "custom-reserve", 1_500)
                .await
                .expect_err("the durable gate must enforce a custom reserve above 1,000"),
            1_512,
            13,
            1_500,
        );
        assert_eq!(github.graph_calls.load(Ordering::SeqCst), 1);

        github.remaining.store(1_001, Ordering::SeqCst);
        services
            .insights
            .repository_contributors("default", "acme/tool", 12)
            .await
            .expect("1,001 remaining admits one contributor request");
        assert_eq!(github.contributor_calls.load(Ordering::SeqCst), 1);

        github.remaining.store(1_000, Ordering::SeqCst);
        assert_rate_budget_error(
            services
                .insights
                .repository_contributors("default", "acme/other", 12)
                .await
                .expect_err("1,000 remaining blocks a contributor request"),
            1_000,
            1,
            1_000,
        );
        assert_eq!(github.contributor_calls.load(Ordering::SeqCst), 1);

        github.remaining.store(1_003, Ordering::SeqCst);
        services
            .insights
            .user_commit_repositories("default", "alice", 12)
            .await
            .expect("1,003 remaining admits three public event pages");
        assert_eq!(github.activity_calls.load(Ordering::SeqCst), 1);

        github.remaining.store(1_002, Ordering::SeqCst);
        assert_rate_budget_error(
            services
                .insights
                .user_commit_repositories("default", "bob", 12)
                .await
                .expect_err("1,002 remaining blocks a three-page event refresh"),
            1_002,
            3,
            1_000,
        );
        assert_eq!(github.activity_calls.load(Ordering::SeqCst), 1);

        github.remaining.store(1_000, Ordering::SeqCst);
        let stale = services
            .insights
            .repository_contributors("default", "acme/stale", 12)
            .await
            .expect("stale contributors remain readable at the reserve floor");
        assert_eq!(stale.contributors[0].login, "cached-contributor");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let failed_refresh = services
            .insights
            .repository_contributors("default", "acme/stale", 12)
            .await
            .expect("failed background refresh keeps stale contributors readable");
        assert_eq!(
            failed_refresh.cache_status(chrono::Utc::now()),
            CacheStatus::RefreshFailed
        );
        assert_eq!(github.contributor_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rate_budget_errors_are_structured_on_rest_and_graphql_surfaces() {
        let (services, _github) = budget_test_services(1_012).await;
        let session_id = services
            .identity
            .create_session("default")
            .await
            .expect("budget-test session");
        let app = crate::http::router(Arc::new(AppState {
            services,
            frontend_origin: "http://localhost:3000".to_string(),
            graph_backend: crate::config::GraphBackend::Memory,
        }));

        let sync_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sync/run")
                    .header("cookie", format!("gitexplore_session={session_id}"))
                    .body(Body::empty())
                    .expect("budgeted sync request"),
            )
            .await
            .expect("budgeted sync response");
        assert_eq!(sync_response.status(), StatusCode::TOO_MANY_REQUESTS);
        let sync_body = to_bytes(sync_response.into_body(), usize::MAX)
            .await
            .expect("budgeted sync body");
        let sync_body: serde_json::Value =
            serde_json::from_slice(&sync_body).expect("budgeted sync json");
        assert_eq!(sync_body["code"], "RATE_BUDGET_RESERVED");
        assert_eq!(sync_body["remaining"], 1_012);
        assert_eq!(sync_body["reserve"], 1_000);
        assert_eq!(sync_body["requested_cost"], 13);
        assert!(sync_body["reset_at"].is_string());

        let graphql_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graphql")
                    .header("cookie", format!("gitexplore_session={session_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "query": "mutation { expandUser(login: \"budget-denied\", limit: 12) { user { login } } }"
                        })
                        .to_string(),
                    ))
                    .expect("budgeted GraphQL request"),
            )
            .await
            .expect("budgeted GraphQL response");
        assert_eq!(graphql_response.status(), StatusCode::OK);
        let graphql_body = to_bytes(graphql_response.into_body(), usize::MAX)
            .await
            .expect("budgeted GraphQL body");
        let graphql_body: serde_json::Value =
            serde_json::from_slice(&graphql_body).expect("budgeted GraphQL json");
        let extensions = &graphql_body["errors"][0]["extensions"];
        assert_eq!(extensions["code"], "RATE_BUDGET_RESERVED");
        assert_eq!(extensions["remaining"], 1_012);
        assert_eq!(extensions["reserve"], 1_000);
        assert_eq!(extensions["requestedCost"], 13);
        assert!(extensions["resetAt"].is_string());
    }

    async fn budget_test_services(remaining: usize) -> (AppServices, Arc<BudgetGitHubClient>) {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .identity
            .save_connection(
                "default",
                GitHubConnection {
                    account: ConnectedAccount {
                        github_user_id: 99,
                        login: "alice".to_string(),
                        display_name: Some("Alice".to_string()),
                    },
                    access_token: "token".to_string(),
                    scopes: vec!["read:user".to_string()],
                },
            )
            .await
            .expect("budget-test connection");
        repositories
            .imports
            .import_github_graph(
                "default",
                GraphImport {
                    viewer: Some(test_user(1, "alice")),
                    followers: vec![test_user(2, "bob")],
                    repositories: vec![
                        test_repository(10, "acme/tool", "Rust", 20, 2),
                        test_repository(11, "acme/other", "Rust", 10, 1),
                        test_repository(12, "acme/stale", "Rust", 5, 0),
                    ],
                    ..Default::default()
                },
            )
            .await
            .expect("budget-test graph seed");
        repositories
            .insights
            .save_repository_contributors(RepositoryContributorInsights::from_snapshot(
                "acme/stale".to_string(),
                RepositoryContributorsSnapshot {
                    contributors: vec![RepositoryContributor {
                        github_id: 90,
                        login: "cached-contributor".to_string(),
                        avatar_url: None,
                        url: "https://github.com/cached-contributor".to_string(),
                        contributions: 7,
                    }],
                    source_complete: true,
                },
                chrono::Utc::now() - chrono::Duration::hours(25),
            ))
            .await
            .expect("budget-test stale insight seed");
        let github = Arc::new(BudgetGitHubClient {
            remaining: AtomicUsize::new(remaining),
            graph_calls: AtomicUsize::new(0),
            contributor_calls: AtomicUsize::new(0),
            activity_calls: AtomicUsize::new(0),
        });
        let services = AppServices::new(
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
            github.clone(),
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );
        (services, github)
    }

    fn assert_rate_budget_error(
        error: AppError,
        remaining: usize,
        requested_cost: usize,
        expected_reserve: usize,
    ) {
        match error {
            AppError::RateBudgetReserved {
                remaining: actual_remaining,
                reserve,
                requested_cost: actual_cost,
                ..
            } => {
                assert_eq!(actual_remaining, remaining);
                assert_eq!(reserve, expected_reserve);
                assert_eq!(actual_cost, requested_cost);
            }
            other => panic!("expected rate-budget reserve error, got {other}"),
        }
    }

    struct BudgetGitHubClient {
        remaining: AtomicUsize,
        graph_calls: AtomicUsize,
        contributor_calls: AtomicUsize,
        activity_calls: AtomicUsize,
    }

    #[async_trait]
    impl GitHubClientPort for BudgetGitHubClient {
        async fn start_device_flow(
            &self,
            _config: &GitHubAuthConfig,
        ) -> AppResult<DeviceLoginStart> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn finish_device_flow(
            &self,
            _config: &GitHubAuthConfig,
            _device_code: &str,
        ) -> AppResult<GitHubConnection> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn exchange_browser_code(
            &self,
            _config: &GitHubAuthConfig,
            _code: &str,
        ) -> AppResult<GitHubConnection> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn fetch_graph(&self, _connection: &GitHubConnection) -> AppResult<GraphImport> {
            self.graph_calls.fetch_add(1, Ordering::SeqCst);
            Ok(GraphImport {
                viewer: Some(test_user(70, "expanded")),
                ..Default::default()
            })
        }

        async fn fetch_user_graph(
            &self,
            connection: &GitHubConnection,
            _login: &str,
        ) -> AppResult<GraphImport> {
            self.fetch_graph(connection).await
        }

        async fn fetch_core_rate_limit(
            &self,
            _connection: &GitHubConnection,
        ) -> AppResult<GitHubRateLimitStatus> {
            let remaining = self.remaining.load(Ordering::SeqCst);
            Ok(GitHubRateLimitStatus {
                limit: 5_000,
                used: 5_000usize.saturating_sub(remaining),
                remaining,
                reset_at: chrono::Utc::now() + chrono::Duration::hours(1),
                checked_at: chrono::Utc::now(),
            })
        }

        async fn fetch_repository_contributors(
            &self,
            _connection: &GitHubConnection,
            _full_name: &str,
        ) -> AppResult<RepositoryContributorsSnapshot> {
            self.contributor_calls.fetch_add(1, Ordering::SeqCst);
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
            self.activity_calls.fetch_add(1, Ordering::SeqCst);
            Ok(UserCommitRepositoriesSnapshot {
                repositories: Vec::new(),
                source_event_count: 0,
                source_truncated: false,
            })
        }

        fn browser_oauth_url(&self, _config: &GitHubAuthConfig, _state: &str) -> AppResult<String> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }
    }

    #[derive(Default)]
    struct InsightGitHubClient {
        rate_limit_calls: AtomicUsize,
        contributor_calls: AtomicUsize,
        activity_calls: AtomicUsize,
    }

    #[async_trait]
    impl GitHubClientPort for InsightGitHubClient {
        async fn start_device_flow(
            &self,
            _config: &GitHubAuthConfig,
        ) -> AppResult<DeviceLoginStart> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn finish_device_flow(
            &self,
            _config: &GitHubAuthConfig,
            _device_code: &str,
        ) -> AppResult<GitHubConnection> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn exchange_browser_code(
            &self,
            _config: &GitHubAuthConfig,
            _code: &str,
        ) -> AppResult<GitHubConnection> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn fetch_graph(&self, _connection: &GitHubConnection) -> AppResult<GraphImport> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn fetch_user_graph(
            &self,
            _connection: &GitHubConnection,
            _login: &str,
        ) -> AppResult<GraphImport> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn fetch_core_rate_limit(
            &self,
            _connection: &GitHubConnection,
        ) -> AppResult<GitHubRateLimitStatus> {
            self.rate_limit_calls.fetch_add(1, Ordering::SeqCst);
            Ok(GitHubRateLimitStatus {
                limit: 5_000,
                used: 120,
                remaining: 4_880,
                reset_at: chrono::Utc::now() + chrono::Duration::hours(1),
                checked_at: chrono::Utc::now(),
            })
        }

        async fn fetch_repository_contributors(
            &self,
            _connection: &GitHubConnection,
            _full_name: &str,
        ) -> AppResult<RepositoryContributorsSnapshot> {
            self.contributor_calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(RepositoryContributorsSnapshot {
                contributors: vec![RepositoryContributor {
                    github_id: 9,
                    login: "octocat".to_string(),
                    avatar_url: Some("https://avatars.example/octocat".to_string()),
                    url: "https://github.com/octocat".to_string(),
                    contributions: 42,
                }],
                source_complete: true,
            })
        }

        async fn fetch_user_commit_repositories(
            &self,
            _connection: &GitHubConnection,
            _login: &str,
        ) -> AppResult<UserCommitRepositoriesSnapshot> {
            self.activity_calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(UserCommitRepositoriesSnapshot {
                repositories: vec![UserCommitRepository {
                    github_id: 2,
                    full_name: "acme/tool".to_string(),
                    url: "https://github.com/acme/tool".to_string(),
                    push_count: 3,
                    commit_count: 7,
                    last_pushed_at: chrono::Utc::now(),
                }],
                source_event_count: 5,
                source_truncated: false,
            })
        }

        fn browser_oauth_url(&self, _config: &GitHubAuthConfig, _state: &str) -> AppResult<String> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }
    }

    struct SharedLeaseImportRepository {
        inner: Arc<dyn GitHubImportRepository>,
        leases: tokio::sync::Mutex<HashMap<String, RefreshLeaseState>>,
    }

    impl SharedLeaseImportRepository {
        fn new(inner: Arc<dyn GitHubImportRepository>) -> Self {
            Self {
                inner,
                leases: tokio::sync::Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl GitHubImportRepository for SharedLeaseImportRepository {
        async fn import_github_graph(
            &self,
            user_id: &str,
            import: GraphImport,
        ) -> AppResult<SyncSummary> {
            self.inner.import_github_graph(user_id, import).await
        }

        async fn try_acquire_refresh_lease(
            &self,
            entity_key: &str,
            token: &str,
            lease_seconds: i64,
        ) -> AppResult<RefreshLeaseAttempt> {
            let mut leases = self.leases.lock().await;
            if let Some(state) = leases
                .get(entity_key)
                .filter(|state| state.status == RefreshLeaseStatus::Running && !state.expired)
            {
                return Ok(RefreshLeaseAttempt::Busy(state.clone()));
            }
            let expires_at = chrono::Utc::now() + chrono::Duration::seconds(lease_seconds);
            leases.insert(
                entity_key.to_string(),
                RefreshLeaseState {
                    status: RefreshLeaseStatus::Running,
                    token: token.to_string(),
                    expires_at: Some(expires_at),
                    expired: false,
                    outcome_json: None,
                    last_error: None,
                },
            );
            Ok(RefreshLeaseAttempt::Acquired(RefreshLease {
                entity_key: entity_key.to_string(),
                token: token.to_string(),
                expires_at,
            }))
        }

        async fn renew_refresh_lease(
            &self,
            lease: &RefreshLease,
            lease_seconds: i64,
        ) -> AppResult<bool> {
            let mut leases = self.leases.lock().await;
            let Some(state) = leases.get_mut(&lease.entity_key) else {
                return Ok(false);
            };
            if state.status != RefreshLeaseStatus::Running || state.token != lease.token {
                return Ok(false);
            }
            state.expires_at = Some(chrono::Utc::now() + chrono::Duration::seconds(lease_seconds));
            Ok(true)
        }

        async fn refresh_lease_state(
            &self,
            entity_key: &str,
        ) -> AppResult<Option<RefreshLeaseState>> {
            Ok(self.leases.lock().await.get(entity_key).cloned())
        }

        async fn complete_refresh_lease(
            &self,
            lease: &RefreshLease,
            outcome_json: Option<&str>,
        ) -> AppResult<bool> {
            let mut leases = self.leases.lock().await;
            let Some(state) = leases.get_mut(&lease.entity_key) else {
                return Ok(false);
            };
            if state.status != RefreshLeaseStatus::Running || state.token != lease.token {
                return Ok(false);
            }
            state.status = RefreshLeaseStatus::Succeeded;
            state.expires_at = None;
            state.outcome_json = outcome_json.map(ToString::to_string);
            Ok(true)
        }

        async fn fail_refresh_lease(&self, lease: &RefreshLease, error: &str) -> AppResult<bool> {
            let mut leases = self.leases.lock().await;
            let Some(state) = leases.get_mut(&lease.entity_key) else {
                return Ok(false);
            };
            if state.status != RefreshLeaseStatus::Running || state.token != lease.token {
                return Ok(false);
            }
            state.status = RefreshLeaseStatus::Failed;
            state.expires_at = None;
            state.last_error = Some(error.to_string());
            Ok(true)
        }

        async fn import_github_graph_under_lease(
            &self,
            user_id: &str,
            import: GraphImport,
            lease: &RefreshLease,
            canonical_login: &str,
        ) -> AppResult<SyncSummary> {
            let summary = self.inner.import_github_graph(user_id, import).await?;
            let outcome = serde_json::to_string(&UserRefreshOutcome {
                canonical_login: canonical_login.to_string(),
                summary: summary.clone(),
            })?;
            if !self.complete_refresh_lease(lease, Some(&outcome)).await? {
                return Err(AppError::External("test lease was lost".to_string()));
            }
            Ok(summary)
        }

        async fn resolve_bookmark_target(&self, target: &BookmarkTarget) -> AppResult<()> {
            self.inner.resolve_bookmark_target(target).await
        }
    }

    struct CountingGitHubClient {
        import: GraphImport,
        user_graph_calls: AtomicUsize,
    }

    #[async_trait]
    impl GitHubClientPort for CountingGitHubClient {
        async fn start_device_flow(
            &self,
            _config: &GitHubAuthConfig,
        ) -> AppResult<DeviceLoginStart> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn finish_device_flow(
            &self,
            _config: &GitHubAuthConfig,
            _device_code: &str,
        ) -> AppResult<GitHubConnection> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn exchange_browser_code(
            &self,
            _config: &GitHubAuthConfig,
            _code: &str,
        ) -> AppResult<GitHubConnection> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }

        async fn fetch_graph(&self, _connection: &GitHubConnection) -> AppResult<GraphImport> {
            Ok(self.import.clone())
        }

        async fn fetch_user_graph(
            &self,
            _connection: &GitHubConnection,
            _login: &str,
        ) -> AppResult<GraphImport> {
            self.user_graph_calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            Ok(self.import.clone())
        }

        async fn fetch_core_rate_limit(
            &self,
            _connection: &GitHubConnection,
        ) -> AppResult<crate::graph::GitHubRateLimitStatus> {
            Ok(GitHubRateLimitStatus {
                limit: 5_000,
                used: 0,
                remaining: 5_000,
                reset_at: chrono::Utc::now() + chrono::Duration::hours(1),
                checked_at: chrono::Utc::now(),
            })
        }

        fn browser_oauth_url(&self, _config: &GitHubAuthConfig, _state: &str) -> AppResult<String> {
            Err(AppError::Unsupported("not used by this test".to_string()))
        }
    }

    fn test_user(github_id: i64, login: &str) -> GitHubUserNode {
        GitHubUserNode {
            github_id,
            login: login.to_string(),
            name: Some(login.to_string()),
            url: format!("https://github.com/{login}"),
            avatar_url: Some(format!("https://avatars.example/{login}")),
            ..Default::default()
        }
    }

    fn test_repository(
        github_id: i64,
        full_name: &str,
        language: &str,
        stargazer_count: u64,
        fork_count: u64,
    ) -> GitHubRepositoryNode {
        let (owner_login, name) = full_name.split_once('/').expect("owner/name");
        GitHubRepositoryNode {
            github_id,
            owner_login: owner_login.to_string(),
            name: name.to_string(),
            full_name: full_name.to_string(),
            description: Some("A useful repository".to_string()),
            html_url: format!("https://github.com/{full_name}"),
            stargazer_count,
            fork_count,
            language: Some(language.to_string()),
            pushed_at: Some(chrono::Utc::now()),
            updated_at: Some(chrono::Utc::now()),
            ..Default::default()
        }
    }

    async fn seeded_http_state() -> AppState {
        let repositories = LocalRepositorySet::in_memory();
        repositories
            .imports
            .import_github_graph(
                "default",
                GraphImport {
                    viewer: Some(GitHubUserNode {
                        github_id: 1,
                        login: "alice".to_string(),
                        name: Some("Alice".to_string()),
                        url: "https://github.com/alice".to_string(),
                        ..Default::default()
                    }),
                    followers: vec![GitHubUserNode {
                        github_id: 2,
                        login: "bob".to_string(),
                        name: None,
                        url: "https://github.com/bob".to_string(),
                        ..Default::default()
                    }],
                    following: vec![GitHubUserNode {
                        github_id: 3,
                        login: "carol".to_string(),
                        name: None,
                        url: "https://github.com/carol".to_string(),
                        ..Default::default()
                    }],
                    starred_repositories: vec![GitHubRepositoryNode {
                        github_id: 10,
                        owner_login: "acme".to_string(),
                        name: "tool".to_string(),
                        full_name: "acme/tool".to_string(),
                        description: None,
                        html_url: "https://github.com/acme/tool".to_string(),
                        ..Default::default()
                    }],
                    repositories: vec![GitHubRepositoryNode {
                        github_id: 11,
                        owner_login: "alice".to_string(),
                        name: "gitexplore".to_string(),
                        full_name: "alice/gitexplore".to_string(),
                        description: None,
                        html_url: "https://github.com/alice/gitexplore".to_string(),
                        ..Default::default()
                    }],
                    coverage: GraphImportCoverage::default(),
                },
            )
            .await
            .expect("seed graph import");

        repositories
            .identity
            .save_connection(
                "default",
                GitHubConnection {
                    account: ConnectedAccount {
                        github_user_id: 1,
                        login: "alice".to_string(),
                        display_name: Some("Alice".to_string()),
                    },
                    access_token: "stub-token".to_string(),
                    scopes: vec!["read:user".to_string()],
                },
            )
            .await
            .expect("seed GitHub connection");

        repositories
            .identity
            .create_session("test-session", "default")
            .await
            .expect("seed session");

        let services = AppServices::new(
            AppServiceRepositories {
                identity: repositories.identity.clone(),
                imports: repositories.imports.clone(),
                sync_state: repositories.sync_state.clone(),
                categories: repositories.categories.clone(),
                bookmarks: repositories.bookmarks.clone(),
                exploration: repositories.exploration.clone(),
                discovery: repositories.discovery.clone(),
                insights: repositories.insights.clone(),
            },
            Arc::new(StubGitHubClient::default()),
            GitHubAuthConfig {
                client_id: secrecy::SecretString::from("stub-client"),
                client_secret: None,
                redirect_uri: None,
                scopes: vec!["read:user".to_string()],
            },
        );

        AppState {
            services,
            frontend_origin: "http://localhost:3000".to_string(),
            graph_backend: crate::config::GraphBackend::Memory,
        }
    }
}
