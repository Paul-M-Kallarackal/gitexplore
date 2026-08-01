use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration as StdDuration,
};

use axum::Router;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::{
    adapters::migrate_json_identity_to_neo4j,
    bookmarks::BookmarkTarget,
    bootstrap::build_app_state,
    config::{AppConfig, GraphBackend},
    discovery::UserNeighborhood,
    exploration::ExplorationSeed,
    graph::{CacheStatus, GitHubRateLimitStatus, GraphImportCoverage},
    http,
    schema::{apply_neo4j_schema, check_neo4j_schema},
    shared::{AppError, AppResult, Shared},
};

#[derive(Debug, Parser)]
#[command(name = "gitexplore")]
pub struct Cli {
    #[arg(long, default_value = "default")]
    pub user_id: String,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Serve,
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
    },
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
    Bookmark {
        #[command(subcommand)]
        command: BookmarkCommand,
    },
    Category {
        #[command(subcommand)]
        command: CategoryCommand,
    },
    Explore {
        #[command(subcommand)]
        command: ExploreCommand,
    },
    Identity {
        #[command(subcommand)]
        command: IdentityCommand,
    },
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    Login {
        #[arg(long, default_value = "device")]
        mode: String,
        #[arg(long)]
        device_code: Option<String>,
    },
    Status,
    Logout,
}

#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    Run,
    Status,
}

#[derive(Debug, Subcommand)]
pub enum GraphCommand {
    /// Fetch and import one public GitHub user's immediate graph.
    Expand {
        #[arg(long)]
        login: String,
    },
    /// Breadth-first import public neighborhoods until the REST reserve or a safety cap is reached.
    Crawl {
        #[arg(long)]
        login: String,
        #[arg(long, default_value_t = crate::application::GITHUB_CORE_REST_RESERVE)]
        request_reserve: usize,
        #[arg(long, default_value_t = 350)]
        max_expansions: usize,
        #[arg(long, default_value_t = 180_000)]
        max_discovered_nodes: usize,
        #[arg(long, default_value_t = 250)]
        delay_ms: u64,
    },
    /// Show the authenticated GitHub token's core REST API budget.
    RateLimit,
}

#[derive(Debug, Subcommand)]
pub enum BookmarkCommand {
    Add {
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, value_delimiter = ',')]
        categories: Vec<String>,
        #[arg(long)]
        note: Option<String>,
    },
    List,
}

#[derive(Debug, Subcommand)]
pub enum CategoryCommand {
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    List,
}

#[derive(Debug, Subcommand)]
pub enum ExploreCommand {
    FromUser { login: String },
    FromRepo { full_name: String },
    FromCategory { name: String },
    Snapshots,
}

#[derive(Debug, Clone, Subcommand)]
pub enum IdentityCommand {
    /// Encrypt and copy the configured identity.json into the configured Neo4j database once.
    MigrateToNeo4j {
        /// Required guard acknowledging the target Neo4j database will be mutated.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum SchemaCommand {
    /// Apply all pending idempotent Neo4j schema migrations.
    Apply,
    /// Verify migration checksums, schema definitions, indexes, and backfills.
    Check,
}

pub async fn run_cli(cli: Cli) -> AppResult<()> {
    let config = AppConfig::from_env()?;
    if let Commands::Identity { command } = &cli.command {
        return identity_command(config, cli.format, command.clone()).await;
    }
    if let Commands::Schema { command } = &cli.command {
        return schema_command(&config, cli.format, command.clone()).await;
    }
    let state = Arc::new(build_app_state(config.clone()).await?);

    match cli.command {
        Commands::Serve => serve(state, config.server_addr).await,
        Commands::Auth { command } => auth_command(state, &cli.user_id, cli.format, command).await,
        Commands::Sync { command } => sync_command(state, &cli.user_id, cli.format, command).await,
        Commands::Graph { command } => {
            graph_command(state, &cli.user_id, cli.format, command).await
        }
        Commands::Bookmark { command } => {
            bookmark_command(state, &cli.user_id, cli.format, command).await
        }
        Commands::Category { command } => {
            category_command(state, &cli.user_id, cli.format, command).await
        }
        Commands::Explore { command } => {
            explore_command(state, &cli.user_id, cli.format, command).await
        }
        Commands::Identity { .. } => unreachable!("identity command handled before bootstrap"),
        Commands::Schema { .. } => unreachable!("schema command handled before bootstrap"),
    }
}

async fn schema_command(
    config: &AppConfig,
    format: OutputFormat,
    command: SchemaCommand,
) -> AppResult<()> {
    if config.graph_backend != GraphBackend::Neo4j {
        return Err(crate::shared::AppError::Config(
            "schema commands require GITEXPLORE_GRAPH_BACKEND=neo4j".to_string(),
        ));
    }
    let report = match command {
        SchemaCommand::Apply => apply_neo4j_schema(&config.neo4j).await?,
        SchemaCommand::Check => check_neo4j_schema(&config.neo4j).await?,
    };
    print_output(format, &serde_json::to_value(report)?);
    Ok(())
}

async fn identity_command(
    config: AppConfig,
    format: OutputFormat,
    command: IdentityCommand,
) -> AppResult<()> {
    match command {
        IdentityCommand::MigrateToNeo4j { confirm } => {
            if !confirm {
                return Err(crate::shared::AppError::Validation(
                    "identity migration requires --confirm".to_string(),
                ));
            }
            if config.graph_backend != GraphBackend::Neo4j {
                return Err(crate::shared::AppError::Config(
                    "identity migration requires GITEXPLORE_GRAPH_BACKEND=neo4j".to_string(),
                ));
            }
            let encryption_key = config.identity_encryption_key.as_ref().ok_or_else(|| {
                crate::shared::AppError::Config(
                    "identity migration requires GITEXPLORE_IDENTITY_ENCRYPTION_KEY".to_string(),
                )
            })?;
            let result = migrate_json_identity_to_neo4j(
                config.identity_store_path(),
                &config.neo4j,
                encryption_key,
            )
            .await?;
            print_output(format, &serde_json::to_value(result)?);
        }
    }
    Ok(())
}

async fn serve(state: Shared<crate::bootstrap::AppState>, addr: String) -> AppResult<()> {
    let app: Router = http::router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .map_err(|error| crate::shared::AppError::External(error.to_string()))
}

async fn auth_command(
    state: Shared<crate::bootstrap::AppState>,
    user_id: &str,
    format: OutputFormat,
    command: AuthCommand,
) -> AppResult<()> {
    match command {
        AuthCommand::Login { mode, device_code } => {
            if mode == "browser" {
                return Err(crate::shared::AppError::Unsupported(
                    "CLI browser login is disabled because OAuth must be bound to the initiating browser; use device login or /auth/oauth/start"
                        .to_string(),
                ));
            }

            if let Some(device_code) = device_code {
                let result = state
                    .services
                    .identity
                    .complete_device_login(user_id, &device_code)
                    .await?;
                print_output(format, &serde_json::json!(result));
            } else {
                let result = state.services.identity.start_device_login(user_id).await?;
                print_output(format, &serde_json::json!(result));
            }
        }
        AuthCommand::Status => {
            let status = state.services.identity.connection_status(user_id).await?;
            print_output(format, &serde_json::json!(status));
        }
        AuthCommand::Logout => {
            state.services.identity.logout(user_id).await?;
            print_output(format, &serde_json::json!({ "ok": true }));
        }
    }
    Ok(())
}

async fn sync_command(
    state: Shared<crate::bootstrap::AppState>,
    user_id: &str,
    format: OutputFormat,
    command: SyncCommand,
) -> AppResult<()> {
    match command {
        SyncCommand::Run => {
            let summary = state.services.sync.run_sync(user_id).await?;
            print_output(format, &serde_json::json!(summary));
        }
        SyncCommand::Status => {
            let status = state.services.sync.status(user_id).await?;
            print_output(format, &serde_json::json!(status));
        }
    }
    Ok(())
}

async fn graph_command(
    state: Shared<crate::bootstrap::AppState>,
    user_id: &str,
    format: OutputFormat,
    command: GraphCommand,
) -> AppResult<()> {
    match command {
        GraphCommand::Expand { login } => {
            let neighborhood = state
                .services
                .discovery
                .expand_user(user_id, &login)
                .await?;
            print_output(
                format,
                &serde_json::to_value(graph_expansion_summary(neighborhood))?,
            );
        }
        GraphCommand::Crawl {
            login,
            request_reserve,
            max_expansions,
            max_discovered_nodes,
            delay_ms,
        } => {
            if request_reserve < crate::application::GITHUB_CORE_REST_RESERVE {
                return Err(AppError::Validation(format!(
                    "request reserve cannot be lower than {}",
                    crate::application::GITHUB_CORE_REST_RESERVE
                )));
            }
            if max_expansions == 0 || max_discovered_nodes == 0 {
                return Err(AppError::Validation(
                    "crawl safety caps must be greater than zero".to_string(),
                ));
            }
            let summary = crawl_graph(
                &state,
                user_id,
                GraphCrawlOptions {
                    seed_login: login,
                    request_reserve,
                    max_expansions,
                    max_discovered_nodes,
                    delay_ms,
                },
            )
            .await?;
            print_output(format, &serde_json::to_value(summary)?);
        }
        GraphCommand::RateLimit => {
            let status = state.services.sync.rate_limit(user_id).await?;
            print_output(format, &serde_json::to_value(status)?);
        }
    }
    Ok(())
}

const CRAWL_EXPANSION_REQUEST_ESTIMATE: usize = 13;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum GraphCrawlStopReason {
    RequestReserveReached,
    DatabaseCapacityReached,
    MaxExpansionsReached,
    NodeSafetyCapReached,
    FrontierExhausted,
}

#[derive(Debug, Serialize)]
struct GraphCrawlCapacityStop {
    resource: String,
    current_count: usize,
    incoming_count: usize,
    projected_count: usize,
    maximum_count: usize,
}

#[derive(Debug, Serialize)]
struct GraphCrawlSummary {
    seed_login: String,
    expansions: usize,
    cached_neighborhoods: usize,
    skipped_users: usize,
    unique_users: usize,
    unique_repositories: usize,
    unique_nodes: usize,
    frontier_remaining: usize,
    request_reserve: usize,
    initial_rate_limit: GitHubRateLimitStatus,
    final_rate_limit: GitHubRateLimitStatus,
    stop_reason: GraphCrawlStopReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    capacity_stop: Option<GraphCrawlCapacityStop>,
}

struct GraphCrawlOptions {
    seed_login: String,
    request_reserve: usize,
    max_expansions: usize,
    max_discovered_nodes: usize,
    delay_ms: u64,
}

async fn crawl_graph(
    state: &Shared<crate::bootstrap::AppState>,
    user_id: &str,
    options: GraphCrawlOptions,
) -> AppResult<GraphCrawlSummary> {
    let GraphCrawlOptions {
        seed_login,
        request_reserve,
        max_expansions,
        max_discovered_nodes,
        delay_ms,
    } = options;
    let seed_login = seed_login.trim().to_ascii_lowercase();
    if seed_login.is_empty() {
        return Err(AppError::Validation(
            "github login cannot be empty".to_string(),
        ));
    }

    let initial_rate_limit = state.services.sync.rate_limit(user_id).await?;
    let mut final_rate_limit = initial_rate_limit.clone();
    let mut frontier = VecDeque::from([seed_login.clone()]);
    let mut queued_logins = HashSet::from([seed_login.clone()]);
    let mut expanded_logins = HashSet::new();
    let mut user_ids = HashSet::new();
    let mut repository_ids = HashSet::new();
    let mut expansions = 0;
    let mut cached_neighborhoods = 0;
    let mut skipped_users = 0;
    let mut consecutive_failures = 0;
    let mut capacity_stop = None;

    let stop_reason = loop {
        if expansions >= max_expansions {
            break GraphCrawlStopReason::MaxExpansionsReached;
        }
        if user_ids.len() + repository_ids.len() >= max_discovered_nodes {
            break GraphCrawlStopReason::NodeSafetyCapReached;
        }
        let Some(next_login) = frontier.pop_front() else {
            break GraphCrawlStopReason::FrontierExhausted;
        };
        if !expanded_logins.insert(next_login.clone()) {
            continue;
        }

        if let Ok(neighborhood) = state
            .services
            .discovery
            .user_neighborhood(user_id, &next_login)
            .await
            && neighborhood.user.neighborhood_cache_status == CacheStatus::Fresh
        {
            cached_neighborhoods += 1;
            collect_crawl_neighborhood(
                &neighborhood,
                &expanded_logins,
                &mut queued_logins,
                &mut frontier,
                &mut user_ids,
                &mut repository_ids,
            );
            continue;
        }
        if final_rate_limit
            .remaining
            .saturating_sub(CRAWL_EXPANSION_REQUEST_ESTIMATE)
            < request_reserve
        {
            break GraphCrawlStopReason::RequestReserveReached;
        }

        let neighborhood = match state
            .services
            .discovery
            .expand_user_with_reserve(user_id, &next_login, request_reserve)
            .await
        {
            Ok(neighborhood) => neighborhood,
            Err(AppError::RateBudgetReserved { .. }) => {
                final_rate_limit = state.services.sync.rate_limit(user_id).await?;
                break GraphCrawlStopReason::RequestReserveReached;
            }
            Err(AppError::GraphCapacityExceeded {
                resource,
                current_count,
                incoming_count,
                projected_count,
                maximum_count,
            }) => {
                capacity_stop = Some(GraphCrawlCapacityStop {
                    resource,
                    current_count,
                    incoming_count,
                    projected_count,
                    maximum_count,
                });
                break GraphCrawlStopReason::DatabaseCapacityReached;
            }
            Err(error) => {
                skipped_users += 1;
                consecutive_failures += 1;
                eprintln!("skipping {next_login}: {error}");
                if consecutive_failures >= 10 {
                    return Err(error);
                }
                if delay_ms > 0 {
                    tokio::time::sleep(StdDuration::from_millis(delay_ms)).await;
                }
                continue;
            }
        };
        expansions += 1;
        consecutive_failures = 0;
        collect_crawl_neighborhood(
            &neighborhood,
            &expanded_logins,
            &mut queued_logins,
            &mut frontier,
            &mut user_ids,
            &mut repository_ids,
        );
        final_rate_limit = state.services.sync.rate_limit(user_id).await?;
        eprintln!(
            "crawl {expansions}/{max_expansions}: {} users, {} repositories, {} core requests remaining",
            user_ids.len(),
            repository_ids.len(),
            final_rate_limit.remaining
        );
        if delay_ms > 0 {
            tokio::time::sleep(StdDuration::from_millis(delay_ms)).await;
        }
    };

    Ok(GraphCrawlSummary {
        seed_login,
        expansions,
        cached_neighborhoods,
        skipped_users,
        unique_users: user_ids.len(),
        unique_repositories: repository_ids.len(),
        unique_nodes: user_ids.len() + repository_ids.len(),
        frontier_remaining: frontier.len(),
        request_reserve,
        initial_rate_limit,
        final_rate_limit,
        stop_reason,
        capacity_stop,
    })
}

fn collect_crawl_neighborhood(
    neighborhood: &UserNeighborhood,
    expanded_logins: &HashSet<String>,
    queued_logins: &mut HashSet<String>,
    frontier: &mut VecDeque<String>,
    user_ids: &mut HashSet<i64>,
    repository_ids: &mut HashSet<i64>,
) {
    user_ids.insert(neighborhood.user.profile.github_id);
    for user in neighborhood
        .following
        .iter()
        .chain(neighborhood.followers.iter())
    {
        user_ids.insert(user.profile.github_id);
        let login = user.profile.login.trim().to_ascii_lowercase();
        if !login.is_empty()
            && !expanded_logins.contains(&login)
            && queued_logins.insert(login.clone())
        {
            frontier.push_back(login);
        }
    }
    repository_ids.extend(
        neighborhood
            .starred_repositories
            .iter()
            .chain(neighborhood.owned_repositories.iter())
            .map(|repository| repository.repository.github_id),
    );
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct GraphExpansionCounts {
    followers: usize,
    following: usize,
    starred_repositories: usize,
    repositories: usize,
    unique_users: usize,
    unique_repositories: usize,
    unique_nodes: usize,
}

#[derive(Debug, Serialize)]
struct GraphExpansionSummary {
    login: String,
    counts: GraphExpansionCounts,
    cache_status: CacheStatus,
    last_fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    coverage: GraphImportCoverage,
}

fn graph_expansion_summary(neighborhood: UserNeighborhood) -> GraphExpansionSummary {
    let mut user_ids = HashSet::new();
    user_ids.insert(neighborhood.user.profile.github_id);
    user_ids.extend(
        neighborhood
            .followers
            .iter()
            .chain(neighborhood.following.iter())
            .map(|user| user.profile.github_id),
    );
    let repository_ids = neighborhood
        .starred_repositories
        .iter()
        .chain(neighborhood.owned_repositories.iter())
        .map(|repository| repository.repository.github_id)
        .collect::<HashSet<_>>();
    let counts = GraphExpansionCounts {
        followers: neighborhood.followers.len(),
        following: neighborhood.following.len(),
        starred_repositories: neighborhood.starred_repositories.len(),
        repositories: neighborhood.owned_repositories.len(),
        unique_users: user_ids.len(),
        unique_repositories: repository_ids.len(),
        unique_nodes: user_ids.len() + repository_ids.len(),
    };

    GraphExpansionSummary {
        login: neighborhood.user.profile.login,
        counts,
        cache_status: neighborhood.user.neighborhood_cache_status,
        last_fetched_at: neighborhood.user.neighborhood_last_fetched_at,
        coverage: neighborhood.coverage,
    }
}

async fn bookmark_command(
    state: Shared<crate::bootstrap::AppState>,
    user_id: &str,
    format: OutputFormat,
    command: BookmarkCommand,
) -> AppResult<()> {
    match command {
        BookmarkCommand::Add {
            user,
            repo,
            categories,
            note,
        } => {
            let target = match (user, repo) {
                (Some(login), None) => BookmarkTarget::GitHubUser { login },
                (None, Some(full_name)) => BookmarkTarget::GitHubRepository { full_name },
                _ => {
                    return Err(crate::shared::AppError::Validation(
                        "provide exactly one of --user or --repo".to_string(),
                    ));
                }
            };
            let bookmark = state
                .services
                .bookmarks
                .add_bookmark(user_id, target, categories, note)
                .await?;
            print_output(format, &serde_json::json!(bookmark));
        }
        BookmarkCommand::List => {
            let bookmarks = state.services.bookmarks.list_bookmarks(user_id).await?;
            print_output(format, &serde_json::json!(bookmarks));
        }
    }
    Ok(())
}

async fn category_command(
    state: Shared<crate::bootstrap::AppState>,
    user_id: &str,
    format: OutputFormat,
    command: CategoryCommand,
) -> AppResult<()> {
    match command {
        CategoryCommand::Create { name, description } => {
            state
                .services
                .bookmarks
                .create_category(user_id, &name, description)
                .await?;
            print_output(format, &serde_json::json!({ "ok": true }));
        }
        CategoryCommand::List => {
            let categories = state.services.bookmarks.list_categories(user_id).await?;
            print_output(format, &serde_json::json!(categories));
        }
    }
    Ok(())
}

async fn explore_command(
    state: Shared<crate::bootstrap::AppState>,
    user_id: &str,
    format: OutputFormat,
    command: ExploreCommand,
) -> AppResult<()> {
    match command {
        ExploreCommand::FromUser { login } => {
            let result = state
                .services
                .exploration
                .explore(user_id, ExplorationSeed::User { login })
                .await?;
            print_output(format, &serde_json::json!(result));
        }
        ExploreCommand::FromRepo { full_name } => {
            let result = state
                .services
                .exploration
                .explore(user_id, ExplorationSeed::Repository { full_name })
                .await?;
            print_output(format, &serde_json::json!(result));
        }
        ExploreCommand::FromCategory { name } => {
            let result = state
                .services
                .exploration
                .explore(user_id, ExplorationSeed::Category { name })
                .await?;
            print_output(format, &serde_json::json!(result));
        }
        ExploreCommand::Snapshots => {
            let snapshots = state.services.exploration.snapshots(user_id).await?;
            print_output(format, &serde_json::json!(snapshots));
        }
    }
    Ok(())
}

fn print_output(format: OutputFormat, value: &serde_json::Value) {
    match format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(value).expect("json output")
        ),
        OutputFormat::Text => println!(
            "{}",
            serde_json::to_string_pretty(value).expect("text output")
        ),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use clap::Parser;

    use super::*;
    use crate::{
        discovery::{DiscoveryRepositoryRecord, DiscoveryUser},
        graph::{GitHubRepositoryNode, GitHubUserNode},
    };

    #[test]
    fn graph_expand_command_parses_an_arbitrary_login() {
        let cli = Cli::try_parse_from([
            "gitexplore",
            "--format",
            "json",
            "graph",
            "expand",
            "--login",
            "octocat",
        ])
        .expect("graph expand command");

        assert!(matches!(cli.format, OutputFormat::Json));
        assert!(matches!(
            cli.command,
            Commands::Graph {
                command: GraphCommand::Expand { login }
            } if login == "octocat"
        ));
    }

    #[test]
    fn graph_rate_limit_command_parses() {
        let cli = Cli::try_parse_from(["gitexplore", "graph", "rate-limit"])
            .expect("graph rate-limit command");
        assert!(matches!(
            cli.command,
            Commands::Graph {
                command: GraphCommand::RateLimit
            }
        ));
    }

    #[test]
    fn graph_crawl_command_preserves_the_minimum_request_reserve() {
        let cli = Cli::try_parse_from(["gitexplore", "graph", "crawl", "--login", "octocat"])
            .expect("graph crawl command");

        assert!(matches!(
            cli.command,
            Commands::Graph {
                command: GraphCommand::Crawl {
                    login,
                    request_reserve: crate::application::GITHUB_CORE_REST_RESERVE,
                    max_expansions: 350,
                    max_discovered_nodes: 180_000,
                    delay_ms: 250,
                }
            } if login == "octocat"
        ));
    }

    #[test]
    fn guarded_identity_migration_command_parses() {
        let cli = Cli::try_parse_from(["gitexplore", "identity", "migrate-to-neo4j", "--confirm"])
            .expect("identity migration command");

        assert!(matches!(
            cli.command,
            Commands::Identity {
                command: IdentityCommand::MigrateToNeo4j { confirm: true }
            }
        ));
    }

    #[test]
    fn schema_apply_and_check_commands_parse() {
        let apply =
            Cli::try_parse_from(["gitexplore", "schema", "apply"]).expect("schema apply command");
        assert!(matches!(
            apply.command,
            Commands::Schema {
                command: SchemaCommand::Apply
            }
        ));

        let check =
            Cli::try_parse_from(["gitexplore", "schema", "check"]).expect("schema check command");
        assert!(matches!(
            check.command,
            Commands::Schema {
                command: SchemaCommand::Check
            }
        ));
    }

    #[test]
    fn expansion_summary_reports_collection_and_unique_node_counts() {
        let fetched_at = Utc::now();
        let shared_person = discovery_user(2, "shared", None);
        let shared_repository = discovery_repository(20, "shared/tool");
        let summary = graph_expansion_summary(UserNeighborhood {
            user: discovery_user(1, "seed", Some(fetched_at)),
            followers: vec![shared_person.clone(), discovery_user(3, "follower", None)],
            following: vec![shared_person, discovery_user(4, "following", None)],
            starred_repositories: vec![
                shared_repository.clone(),
                discovery_repository(21, "other/starred"),
            ],
            owned_repositories: vec![shared_repository],
            coverage: GraphImportCoverage::default(),
        });

        assert_eq!(summary.login, "seed");
        assert_eq!(
            summary.counts,
            GraphExpansionCounts {
                followers: 2,
                following: 2,
                starred_repositories: 2,
                repositories: 1,
                unique_users: 4,
                unique_repositories: 2,
                unique_nodes: 6,
            }
        );
        assert_eq!(summary.last_fetched_at, Some(fetched_at));
        assert_eq!(summary.coverage, GraphImportCoverage::default());
    }

    #[test]
    fn crawl_frontier_prioritizes_following_and_deduplicates_people_and_repositories() {
        let shared_person = discovery_user(2, "Shared", None);
        let shared_repository = discovery_repository(20, "shared/tool");
        let neighborhood = UserNeighborhood {
            user: discovery_user(1, "Seed", None),
            followers: vec![shared_person.clone(), discovery_user(3, "Follower", None)],
            following: vec![shared_person, discovery_user(4, "Following", None)],
            starred_repositories: vec![shared_repository.clone()],
            owned_repositories: vec![shared_repository],
            coverage: GraphImportCoverage::default(),
        };
        let expanded = HashSet::from(["seed".to_string()]);
        let mut queued = HashSet::from(["seed".to_string()]);
        let mut frontier = VecDeque::new();
        let mut users = HashSet::new();
        let mut repositories = HashSet::new();

        collect_crawl_neighborhood(
            &neighborhood,
            &expanded,
            &mut queued,
            &mut frontier,
            &mut users,
            &mut repositories,
        );

        assert_eq!(
            frontier.into_iter().collect::<Vec<_>>(),
            vec![
                "shared".to_string(),
                "following".to_string(),
                "follower".to_string()
            ]
        );
        assert_eq!(users, HashSet::from([1, 2, 3, 4]));
        assert_eq!(repositories, HashSet::from([20]));
    }

    fn discovery_user(
        github_id: i64,
        login: &str,
        fetched_at: Option<chrono::DateTime<Utc>>,
    ) -> DiscoveryUser {
        DiscoveryUser {
            profile: GitHubUserNode {
                github_id,
                login: login.to_string(),
                url: format!("https://github.com/{login}"),
                ..Default::default()
            },
            neighborhood_cache_status: if fetched_at.is_some() {
                CacheStatus::Fresh
            } else {
                CacheStatus::Stale
            },
            neighborhood_last_fetched_at: fetched_at,
        }
    }

    fn discovery_repository(github_id: i64, full_name: &str) -> DiscoveryRepositoryRecord {
        let (owner_login, name) = full_name.split_once('/').expect("repository full name");
        DiscoveryRepositoryRecord {
            repository: GitHubRepositoryNode {
                github_id,
                owner_login: owner_login.to_string(),
                name: name.to_string(),
                full_name: full_name.to_string(),
                html_url: format!("https://github.com/{full_name}"),
                ..Default::default()
            },
            saved: false,
        }
    }
}
