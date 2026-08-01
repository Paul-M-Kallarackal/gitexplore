CREATE CONSTRAINT local_user_id IF NOT EXISTS
FOR (n:LocalUser)
REQUIRE n.id IS UNIQUE;

CREATE CONSTRAINT gitexplore_schema_migration_version IF NOT EXISTS
FOR (n:GitExploreSchemaMigration)
REQUIRE n.version IS UNIQUE;

CREATE CONSTRAINT refresh_lease_entity_key IF NOT EXISTS
FOR (n:RefreshLease)
REQUIRE n.entity_key IS UNIQUE;

CREATE CONSTRAINT github_identity_github_user_id IF NOT EXISTS
FOR (n:GitHubIdentity)
REQUIRE n.github_user_id IS UNIQUE;

CREATE CONSTRAINT github_identity_user_id IF NOT EXISTS
FOR (n:GitHubIdentity)
REQUIRE n.user_id IS UNIQUE;

CREATE CONSTRAINT oauth_pending_state_digest IF NOT EXISTS
FOR (n:OAuthPendingState)
REQUIRE n.state_digest IS UNIQUE;

CREATE CONSTRAINT browser_session_id_digest IF NOT EXISTS
FOR (n:BrowserSession)
REQUIRE n.id_digest IS UNIQUE;

CREATE CONSTRAINT github_user_id IF NOT EXISTS
FOR (n:User)
REQUIRE n.github_id IS UNIQUE;

CREATE CONSTRAINT github_repo_id IF NOT EXISTS
FOR (n:Repository)
REQUIRE n.github_id IS UNIQUE;

MATCH (user:User)
WHERE user.login IS NOT NULL
WITH toLower(user.login) AS alias, user
ORDER BY coalesce(user.neighborhood_last_fetched_at, datetime({epochMillis: 0})) DESC, user.github_id DESC
WITH alias, collect(user) AS users
WHERE size(users) > 1
UNWIND tail(users) AS duplicate
SET duplicate.login = '__gitexplore-user-' + toString(duplicate.github_id)
WITH duplicate
OPTIONAL MATCH (duplicate)-[:OWNS]->(owned:Repository)
SET owned.owner_login = duplicate.login;

MATCH (user:User)
WHERE user.login IS NOT NULL
SET user.login_key = toLower(user.login);

CREATE CONSTRAINT github_user_login_key IF NOT EXISTS
FOR (n:User)
REQUIRE n.login_key IS UNIQUE;

MATCH (user:User)
WHERE user.followers_complete IS NULL
   OR user.following_complete IS NULL
   OR user.starred_repositories_complete IS NULL
   OR user.repositories_complete IS NULL
SET user.followers_complete = coalesce(user.followers_complete, false),
    user.following_complete = coalesce(user.following_complete, false),
    user.starred_repositories_complete = coalesce(user.starred_repositories_complete, false),
    user.repositories_complete = coalesce(user.repositories_complete, false),
    user.neighborhood_stale_at = datetime({epochMillis: 0});

MATCH (repo:Repository)
WHERE repo.full_name IS NOT NULL
WITH toLower(repo.full_name) AS alias, repo
ORDER BY coalesce(repo.last_fetched_at, datetime({epochMillis: 0})) DESC, repo.github_id DESC
WITH alias, collect(repo) AS repositories
WHERE size(repositories) > 1
FOREACH (duplicate IN tail(repositories) |
  SET duplicate.full_name = '__gitexplore-repository-' + toString(duplicate.github_id)
);

MATCH (repo:Repository)
WHERE repo.full_name IS NOT NULL
SET repo.full_name_key = toLower(repo.full_name);

CREATE CONSTRAINT github_repository_full_name_key IF NOT EXISTS
FOR (n:Repository)
REQUIRE n.full_name_key IS UNIQUE;

CREATE CONSTRAINT sync_state_user_id IF NOT EXISTS
FOR (n:SyncState)
REQUIRE n.user_id IS UNIQUE;

CREATE CONSTRAINT bookmark_id IF NOT EXISTS
FOR (n:Bookmark)
REQUIRE n.id IS UNIQUE;

MATCH (bookmark:Bookmark)-[:TARGETS_USER]->(target:User)
WHERE bookmark.user_id IS NOT NULL
SET bookmark.target_kind = 'github-user',
    bookmark.target_github_id = target.github_id;

MATCH (bookmark:Bookmark)-[:TARGETS_REPO]->(target:Repository)
WHERE bookmark.user_id IS NOT NULL
SET bookmark.target_kind = 'github-repository',
    bookmark.target_github_id = target.github_id;

CREATE CONSTRAINT bookmark_owner_target_id IF NOT EXISTS
FOR (n:Bookmark)
REQUIRE (n.user_id, n.target_kind, n.target_github_id) IS UNIQUE;

CREATE CONSTRAINT exploration_snapshot_id IF NOT EXISTS
FOR (n:ExplorationSnapshot)
REQUIRE n.id IS UNIQUE;

CREATE CONSTRAINT category_owner_name IF NOT EXISTS
FOR (n:Category)
REQUIRE (n.user_id, n.name) IS UNIQUE;

CREATE INDEX github_user_login IF NOT EXISTS
FOR (n:User)
ON (n.login);

CREATE INDEX github_repository_full_name IF NOT EXISTS
FOR (n:Repository)
ON (n.full_name);

CREATE INDEX github_repository_language IF NOT EXISTS
FOR (n:Repository)
ON (n.language);

CREATE INDEX github_repository_stargazer_count IF NOT EXISTS
FOR (n:Repository)
ON (n.stargazer_count);

CREATE INDEX github_user_neighborhood_stale_at IF NOT EXISTS
FOR (n:User)
ON (n.neighborhood_stale_at);

CREATE INDEX oauth_pending_state_expires_at IF NOT EXISTS
FOR (n:OAuthPendingState)
ON (n.expires_at);

CREATE INDEX browser_session_expires_at IF NOT EXISTS
FOR (n:BrowserSession)
ON (n.expires_at);

CREATE INDEX refresh_lease_expires_at IF NOT EXISTS
FOR (n:RefreshLease)
ON (n.expires_at);

CREATE INDEX github_repository_contributors_stale_at IF NOT EXISTS
FOR (n:Repository)
ON (n.contributors_stale_at);

CREATE INDEX github_user_commit_activity_stale_at IF NOT EXISTS
FOR (n:User)
ON (n.commit_activity_stale_at);

CREATE INDEX github_repository_contribution_count IF NOT EXISTS
FOR ()-[relationship:CONTRIBUTED_TO]-()
ON (relationship.contributions);

CREATE INDEX github_user_recent_commit_count IF NOT EXISTS
FOR ()-[relationship:RECENTLY_PUSHED_TO]-()
ON (relationship.commit_count);
