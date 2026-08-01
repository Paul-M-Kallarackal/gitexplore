use std::collections::HashMap;

use neo4rs::query;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    adapters::Neo4jClient,
    config::Neo4jConfig,
    shared::{AppError, AppResult},
};

const BASELINE_VERSION: i64 = 1;
const BASELINE_SCHEMA: &str = include_str!("../docker/neo4j/init/01-schema.cypher");

#[derive(Debug, Clone, Serialize)]
pub struct SchemaReport {
    pub version: i64,
    pub checksum: String,
    pub statements: usize,
    pub applied: bool,
}

#[derive(Clone, Copy)]
struct ExpectedSchemaEntry {
    name: &'static str,
    entity_type: &'static str,
    label_or_type: &'static str,
    properties: &'static [&'static str],
}

const EXPECTED_CONSTRAINTS: &[ExpectedSchemaEntry] = &[
    constraint("local_user_id", "LocalUser", &["id"]),
    constraint(
        "gitexplore_schema_migration_version",
        "GitExploreSchemaMigration",
        &["version"],
    ),
    constraint("refresh_lease_entity_key", "RefreshLease", &["entity_key"]),
    constraint(
        "github_identity_github_user_id",
        "GitHubIdentity",
        &["github_user_id"],
    ),
    constraint("github_identity_user_id", "GitHubIdentity", &["user_id"]),
    constraint(
        "oauth_pending_state_digest",
        "OAuthPendingState",
        &["state_digest"],
    ),
    constraint(
        "browser_session_id_digest",
        "BrowserSession",
        &["id_digest"],
    ),
    constraint("github_user_id", "User", &["github_id"]),
    constraint("github_repo_id", "Repository", &["github_id"]),
    constraint("github_user_login_key", "User", &["login_key"]),
    constraint(
        "github_repository_full_name_key",
        "Repository",
        &["full_name_key"],
    ),
    constraint("sync_state_user_id", "SyncState", &["user_id"]),
    constraint("bookmark_id", "Bookmark", &["id"]),
    constraint(
        "bookmark_owner_target_id",
        "Bookmark",
        &["user_id", "target_kind", "target_github_id"],
    ),
    constraint("exploration_snapshot_id", "ExplorationSnapshot", &["id"]),
    constraint("category_owner_name", "Category", &["user_id", "name"]),
];

const EXPECTED_INDEXES: &[ExpectedSchemaEntry] = &[
    index("github_user_login", "NODE", "User", &["login"]),
    index(
        "github_repository_full_name",
        "NODE",
        "Repository",
        &["full_name"],
    ),
    index(
        "github_repository_language",
        "NODE",
        "Repository",
        &["language"],
    ),
    index(
        "github_repository_stargazer_count",
        "NODE",
        "Repository",
        &["stargazer_count"],
    ),
    index(
        "github_user_neighborhood_stale_at",
        "NODE",
        "User",
        &["neighborhood_stale_at"],
    ),
    index(
        "oauth_pending_state_expires_at",
        "NODE",
        "OAuthPendingState",
        &["expires_at"],
    ),
    index(
        "browser_session_expires_at",
        "NODE",
        "BrowserSession",
        &["expires_at"],
    ),
    index(
        "refresh_lease_expires_at",
        "NODE",
        "RefreshLease",
        &["expires_at"],
    ),
    index(
        "github_repository_contributors_stale_at",
        "NODE",
        "Repository",
        &["contributors_stale_at"],
    ),
    index(
        "github_user_commit_activity_stale_at",
        "NODE",
        "User",
        &["commit_activity_stale_at"],
    ),
    index(
        "github_repository_contribution_count",
        "RELATIONSHIP",
        "CONTRIBUTED_TO",
        &["contributions"],
    ),
    index(
        "github_user_recent_commit_count",
        "RELATIONSHIP",
        "RECENTLY_PUSHED_TO",
        &["commit_count"],
    ),
];

const fn constraint(
    name: &'static str,
    label: &'static str,
    properties: &'static [&'static str],
) -> ExpectedSchemaEntry {
    ExpectedSchemaEntry {
        name,
        entity_type: "NODE",
        label_or_type: label,
        properties,
    }
}

const fn index(
    name: &'static str,
    entity_type: &'static str,
    label_or_type: &'static str,
    properties: &'static [&'static str],
) -> ExpectedSchemaEntry {
    ExpectedSchemaEntry {
        name,
        entity_type,
        label_or_type,
        properties,
    }
}

pub async fn apply_neo4j_schema(config: &Neo4jConfig) -> AppResult<SchemaReport> {
    let client = Neo4jClient::new(config).await?;
    apply_neo4j_schema_client(&client).await
}

pub async fn check_neo4j_schema(config: &Neo4jConfig) -> AppResult<SchemaReport> {
    let client = Neo4jClient::new(config).await?;
    check_neo4j_schema_client(&client).await
}

pub(crate) async fn apply_neo4j_schema_client(client: &Neo4jClient) -> AppResult<SchemaReport> {
    client
        .run(query(
            "CREATE CONSTRAINT gitexplore_schema_migration_version IF NOT EXISTS
             FOR (n:GitExploreSchemaMigration)
             REQUIRE n.version IS UNIQUE",
        ))
        .await?;

    let token = Uuid::new_v4().to_string();
    if !acquire_migration_lease(client, &token).await? {
        return Err(AppError::Storage(
            "another Neo4j schema migration is already running".to_string(),
        ));
    }

    let result = apply_locked(client).await;
    let release = release_migration_lease(client, &token).await;
    match (result, release) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(report), Ok(())) => Ok(report),
    }
}

async fn apply_locked(client: &Neo4jClient) -> AppResult<SchemaReport> {
    let checksum = schema_checksum(BASELINE_SCHEMA);
    let existing = migration_checksum(client, BASELINE_VERSION).await?;
    if let Some(existing) = existing.as_ref()
        && existing != &checksum
    {
        return Err(AppError::Storage(format!(
            "Neo4j schema migration {BASELINE_VERSION} checksum mismatch"
        )));
    }

    let statements = schema_statements(BASELINE_SCHEMA);
    let applied = existing.is_none();
    if applied {
        for statement in &statements {
            client.run(query(statement)).await?;
        }
        client
            .run(
                query(
                    "MERGE (migration:GitExploreSchemaMigration {version: $version})
                     SET migration.checksum = $checksum,
                         migration.applied_at = datetime()",
                )
                .param("version", BASELINE_VERSION)
                .param("checksum", checksum.clone()),
            )
            .await?;
    }
    check_schema_definitions(client, &checksum).await?;
    Ok(SchemaReport {
        version: BASELINE_VERSION,
        checksum,
        statements: statements.len(),
        applied,
    })
}

pub(crate) async fn check_neo4j_schema_client(client: &Neo4jClient) -> AppResult<SchemaReport> {
    let checksum = schema_checksum(BASELINE_SCHEMA);
    let stored = migration_checksum(client, BASELINE_VERSION)
        .await?
        .ok_or_else(|| {
            AppError::Storage(format!(
                "Neo4j schema migration {BASELINE_VERSION} has not been applied"
            ))
        })?;
    if stored != checksum {
        return Err(AppError::Storage(format!(
            "Neo4j schema migration {BASELINE_VERSION} checksum mismatch"
        )));
    }
    check_schema_definitions(client, &checksum).await?;
    Ok(SchemaReport {
        version: BASELINE_VERSION,
        checksum,
        statements: schema_statements(BASELINE_SCHEMA).len(),
        applied: false,
    })
}

async fn acquire_migration_lease(client: &Neo4jClient, token: &str) -> AppResult<bool> {
    let mut rows = client
        .graph
        .execute_on(
            &client.database,
            query(
                "MERGE (lock:GitExploreSchemaMigration {version: 0})
                 ON CREATE SET lock.checksum = 'migration-lock', lock.mutex = 0
                 SET lock.mutex = coalesce(lock.mutex, 0) + 1
                 WITH lock,
                      lock.lock_token IS NULL
                        OR lock.lock_expires_at IS NULL
                        OR lock.lock_expires_at <= datetime() AS available
                 FOREACH (_ IN CASE WHEN available THEN [1] ELSE [] END |
                   SET lock.lock_token = $token,
                       lock.lock_expires_at = datetime() + duration({minutes: 30})
                 )
                 RETURN available AS acquired",
            )
            .param("token", token.to_string()),
        )
        .await
        .map_err(|error| AppError::External(error.to_string()))?;
    let row = rows
        .next()
        .await
        .map_err(|error| AppError::External(error.to_string()))?
        .ok_or_else(|| AppError::Storage("schema lock query returned no row".to_string()))?;
    row.get::<bool>("acquired")
        .map_err(|error| AppError::Storage(error.to_string()))
}

async fn release_migration_lease(client: &Neo4jClient, token: &str) -> AppResult<()> {
    client
        .run(
            query(
                "MATCH (lock:GitExploreSchemaMigration {version: 0})
                 WHERE lock.lock_token = $token
                 SET lock.lock_token = null,
                     lock.lock_expires_at = null",
            )
            .param("token", token.to_string()),
        )
        .await
}

async fn migration_checksum(client: &Neo4jClient, version: i64) -> AppResult<Option<String>> {
    let mut rows = client
        .graph
        .execute_on(
            &client.database,
            query(
                "MATCH (migration:GitExploreSchemaMigration {version: $version})
                 RETURN migration.checksum AS checksum",
            )
            .param("version", version),
        )
        .await
        .map_err(|error| AppError::External(error.to_string()))?;
    rows.next()
        .await
        .map_err(|error| AppError::External(error.to_string()))?
        .map(|row| {
            row.get::<String>("checksum")
                .map_err(|error| AppError::Storage(error.to_string()))
        })
        .transpose()
}

async fn check_schema_definitions(client: &Neo4jClient, checksum: &str) -> AppResult<()> {
    let constraint_names = EXPECTED_CONSTRAINTS
        .iter()
        .map(|entry| entry.name.to_string())
        .collect::<Vec<_>>();
    let mut rows = client
        .graph
        .execute_on(
            &client.database,
            query(
                "SHOW CONSTRAINTS
                 YIELD name, type, entityType, labelsOrTypes, properties
                 WHERE name IN $names
                 RETURN name, type, entityType, labelsOrTypes, properties",
            )
            .param("names", constraint_names),
        )
        .await
        .map_err(|error| AppError::External(error.to_string()))?;
    let mut constraints = HashMap::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|error| AppError::External(error.to_string()))?
    {
        constraints.insert(
            row.get::<String>("name")
                .map_err(|error| AppError::Storage(error.to_string()))?,
            (
                row.get::<String>("type")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
                row.get::<String>("entityType")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
                row.get::<Vec<String>>("labelsOrTypes")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
                row.get::<Vec<String>>("properties")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
            ),
        );
    }
    for expected in EXPECTED_CONSTRAINTS {
        let Some((kind, entity_type, labels, properties)) = constraints.get(expected.name) else {
            return Err(AppError::Storage(format!(
                "Neo4j constraint `{}` is missing",
                expected.name
            )));
        };
        let expected_properties = expected
            .properties
            .iter()
            .map(|property| (*property).to_string())
            .collect::<Vec<_>>();
        if kind != "UNIQUENESS"
            || entity_type != expected.entity_type
            || labels.as_slice() != [expected.label_or_type]
            || properties != &expected_properties
        {
            return Err(AppError::Storage(format!(
                "Neo4j constraint `{}` has an unexpected definition",
                expected.name
            )));
        }
    }

    let index_names = EXPECTED_INDEXES
        .iter()
        .map(|entry| entry.name.to_string())
        .collect::<Vec<_>>();
    let mut rows = client
        .graph
        .execute_on(
            &client.database,
            query(
                "SHOW INDEXES
                 YIELD name, type, entityType, labelsOrTypes, properties, state
                 WHERE name IN $names
                 RETURN name, type, entityType, labelsOrTypes, properties, state",
            )
            .param("names", index_names),
        )
        .await
        .map_err(|error| AppError::External(error.to_string()))?;
    let mut indexes = HashMap::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|error| AppError::External(error.to_string()))?
    {
        indexes.insert(
            row.get::<String>("name")
                .map_err(|error| AppError::Storage(error.to_string()))?,
            (
                row.get::<String>("type")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
                row.get::<String>("entityType")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
                row.get::<Vec<String>>("labelsOrTypes")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
                row.get::<Vec<String>>("properties")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
                row.get::<String>("state")
                    .map_err(|error| AppError::Storage(error.to_string()))?,
            ),
        );
    }
    for expected in EXPECTED_INDEXES {
        let Some((kind, entity_type, labels, properties, state)) = indexes.get(expected.name)
        else {
            return Err(AppError::Storage(format!(
                "Neo4j index `{}` is missing",
                expected.name
            )));
        };
        let expected_properties = expected
            .properties
            .iter()
            .map(|property| (*property).to_string())
            .collect::<Vec<_>>();
        if kind != "RANGE"
            || entity_type != expected.entity_type
            || labels.as_slice() != [expected.label_or_type]
            || properties != &expected_properties
            || state != "ONLINE"
        {
            return Err(AppError::Storage(format!(
                "Neo4j index `{}` is not online with the expected definition",
                expected.name
            )));
        }
    }

    check_zero_count(
        client,
        "MATCH (user:User)
         WHERE user.login IS NOT NULL
           AND (user.login_key IS NULL OR user.login_key <> toLower(user.login))
         RETURN count(user) AS invalid",
        "user login keys",
    )
    .await?;
    check_zero_count(
        client,
        "MATCH (repo:Repository)
         WHERE repo.full_name IS NOT NULL
           AND (repo.full_name_key IS NULL OR repo.full_name_key <> toLower(repo.full_name))
         RETURN count(repo) AS invalid",
        "repository full-name keys",
    )
    .await?;

    let stored = migration_checksum(client, BASELINE_VERSION).await?;
    if stored.as_deref() != Some(checksum) {
        return Err(AppError::Storage(format!(
            "Neo4j schema migration {BASELINE_VERSION} checksum mismatch"
        )));
    }
    Ok(())
}

async fn check_zero_count(client: &Neo4jClient, cypher: &str, label: &str) -> AppResult<()> {
    let mut rows = client
        .graph
        .execute_on(&client.database, query(cypher))
        .await
        .map_err(|error| AppError::External(error.to_string()))?;
    let invalid = rows
        .next()
        .await
        .map_err(|error| AppError::External(error.to_string()))?
        .and_then(|row| row.get::<i64>("invalid").ok())
        .unwrap_or_default();
    if invalid == 0 {
        Ok(())
    } else {
        Err(AppError::Storage(format!(
            "Neo4j schema check found {invalid} invalid {label}"
        )))
    }
}

pub(crate) fn schema_statements(schema: &str) -> Vec<&str> {
    schema
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .collect()
}

pub(crate) fn schema_checksum(schema: &str) -> String {
    Sha256::digest(schema.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{BASELINE_SCHEMA, schema_checksum, schema_statements};

    #[test]
    fn embedded_schema_has_stable_nonempty_statements_and_checksum() {
        let statements = schema_statements(BASELINE_SCHEMA);
        assert!(statements.len() >= 25);
        assert!(statements.iter().all(|statement| !statement.ends_with(';')));
        let checksum = schema_checksum(BASELINE_SCHEMA);
        assert_eq!(checksum.len(), 64);
        assert!(
            checksum
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
        assert_eq!(
            checksum, "200b4087d22e330c1b18965083eaddf5b6a8d89982654ecef6b17ff11e8db4f0",
            "migration v1 is immutable; add a new ordered migration instead"
        );
    }
}
