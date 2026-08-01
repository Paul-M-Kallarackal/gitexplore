# Graph Explorer and GraphQL

This document is the source of truth for the implemented click-through graph explorer. It covers the browser workflow, GraphQL contract, public expansion semantics, repository ranking, and compatibility with the older REST workflows.

## Authentication boundary

The explorer uses `POST /graphql`. The Rust HTTP adapter resolves the app user only from the opaque, server-managed `gitexplore_session` cookie and injects that identity into the GraphQL request context.

```http
POST /graphql HTTP/1.1
Host: localhost:4000
Content-Type: application/json
Cookie: gitexplore_session=<opaque-session-id>

{"query":"query Neighborhood($login: String!, $limit: Int!) { neighborhood(login: $login, limit: $limit) { user { login } cacheStatus coverage { followersComplete followingComplete starredRepositoriesComplete repositoriesComplete } } }","variables":{"login":"octocat","limit":24}}
```

- The browser receives the `HttpOnly; SameSite=Lax; Path=/` cookie after GitHub OAuth completes.
- OAuth start creates opaque, one-time server-side state with a 10-minute lifetime and binds it to an `HttpOnly`, `SameSite=Lax` nonce cookie scoped to `/auth/oauth`. Pending state is durable through the active identity repository, capped at 256 entries, and consumable by any Neo4j-backed service replica.
- Reconnecting the same stable GitHub account reuses its canonical app-user identity and private overlay.
- Durable session records and their cookies expire after 30 days. Neo4j stores only keyed session-id digests; expired records are purged during session creation/resolution, and the store is bounded to 4,096 active sessions.
- `POST /auth/logout` removes the presented server-side session mapping and expires the browser cookie.
- The typed browser client sends requests with `credentials: "include"`.
- GraphQL does not accept `user_id` as an argument, variable, query parameter, or header.
- A missing or unresolved session returns a GraphQL `UNAUTHENTICATED` error.
- `limit` must be between `1` and `100`.

## Operations

### Read a known neighborhood

`neighborhood` reads the shared cache. It does not call GitHub or start a background job.

```graphql
query Neighborhood($login: String!, $limit: Int!) {
  neighborhood(login: $login, limit: $limit) {
    user {
      githubId
      login
      name
      url
      avatarUrl
    }
    followers {
      login
      name
      avatarUrl
    }
    following {
      login
      name
      avatarUrl
    }
    repositories {
      repository {
        fullName
        description
        htmlUrl
        stargazerCount
        forkCount
        primaryLanguage
        updatedAt
        archived
        fork
      }
      networkStars
      viaLogins
      discoveryScore
      reasons
      saved
    }
    cacheStatus
    lastFetchedAt
    coverage {
      followersComplete
      followingComplete
      starredRepositoriesComplete
      repositoriesComplete
    }
  }
}
```

`followers` and `following` are deliberately separate. `repositories` combines repositories directly starred or owned by the current node with ranked repositories reached through that node's immediate follower/following network.

### Expand or refresh a user

`expandUser` synchronously fetches the requested public GitHub graph with the authenticated app user's GitHub connection and returns the updated neighborhood. Complete collections replace their corresponding cached relationships; capped partial collections merge with and preserve prior relationships.

```graphql
mutation ExpandUser($login: String!, $limit: Int!) {
  expandUser(login: $login, limit: $limit) {
    user {
      login
    }
    followers {
      login
    }
    following {
      login
    }
    repositories {
      repository {
        fullName
      }
      discoveryScore
      reasons
      saved
    }
    cacheStatus
    lastFetchedAt
    coverage {
      followersComplete
      followingComplete
      starredRepositoriesComplete
      repositoriesComplete
    }
  }
}
```

Expansion calls these public GitHub endpoints:

- `/users/:login`
- `/users/:login/followers`
- `/users/:login/following`
- `/users/:login/starred`
- `/users/:login/repos`

The profile is fetched once. The followers, following, starred repositories, and owned repositories collections are each capped independently at 300 GitHub entries per expansion. Repository entries are then filtered to public visibility, so an imported repository collection can contain fewer than 300 items.

The GraphQL `coverage` object makes the cap semantics explicit:

- `followersComplete`
- `followingComplete`
- `starredRepositoriesComplete`
- `repositoriesComplete`

A field is `true` when pagination finished within the cap. It is `false` when the collection reached 300 entries and GitHub still exposed another page. Public-repository filtering does not itself make coverage partial.

Legacy cached users with no stored coverage fields report all four values as `false` and remain `STALE` until refreshed. Missing historical metadata is never interpreted as proof of complete coverage.

The import preserves direction:

```text
(follower)-[:FOLLOWS]->(expanded user)-[:FOLLOWS]->(followed account)
(expanded user)-[:STARRED]->(repository)
(expanded user)-[:OWNS]->(repository)
```

Concurrent expansion calls deduplicate on the case-normalized key `github-user:<login>`. A process-local flight avoids unnecessary database polling, and Neo4j deployments additionally coordinate replicas through a five-minute `RefreshLease`. The lease uses Neo4j server time, an opaque fencing token, and one-minute heartbeat renewal. The graph import locks and validates the token and commits the success outcome with the graph transaction, so an expired owner cannot overwrite its replacement. A waiter reuses the completed outcome; a later explicit sequential refresh runs again. File mode retains the cancellation-safe in-process behavior.

After winning that entity lease, the refresher acquires a second durable lease scoped to the connected account's stable `GitHubIdentity` and probes GitHub's REST `core` budget. User expansion is admitted only when its conservative 13-request maximum would leave at least 1,000 requests. Repository contributor and public event refreshes reserve one and three requests respectively. This ordering avoids spending or reserving an account budget for a duplicate entity refresh.

If the reserve blocks an explicit expansion or a cold insight fetch, GraphQL returns `RATE_BUDGET_RESERVED` with the observed remaining count, the 1,000 reserve, the operation cost, and GitHub's UTC reset time. Cache-only `neighborhood` reads and private `saveRepository` mutations do not consume this crawl budget. A stale insight remains visible while a rejected background refresh records its failure.

Neighbor nodes discovered during somebody else's expansion are not marked as having a fresh neighborhood. Their `lastFetchedAt` remains absent until that specific login is expanded.

Public graph lookups use lowercase normalized aliases, but user and repository identity follows GitHub's stable numeric ids. A case-only or ordinary rename moves the same node, relationships, coverage, and saves to the new alias. If GitHub reuses an old login or repository full name for another numeric id, GitExplore keeps both histories separate and moves the displaced entity to a reserved stable-id placeholder.

For each collection with complete coverage, GitExplore deletes the corresponding prior `FOLLOWS`, `STARRED`, or `OWNS` edges and inserts the current authoritative set. For each partial collection, it preserves existing edges and merges the fetched entries, avoiding destructive replacement from a capped prefix. If any collection is partial, the neighborhood remains `STALE`. Neo4j performs the complete replacements and partial merges in one transaction; a failure rolls back the entire import.

### Warm the discovery graph

`startDiscoveryWarmup` starts one private job for the authenticated app user; it accepts no identity or seed argument. The connected GitHub login is the seed. Repeated starts return the same active job, so browser retries cannot create duplicate work. `COMPLETED` remains idempotently complete. A `RESERVE_PROTECTED` job is also returned unchanged before its recorded reset; an explicit start after that reset atomically requeues the same job id with its existing frontier and progress. A start after `FAILED` creates a new attempt.

```graphql
mutation StartDiscoveryWarmup {
  startDiscoveryWarmup {
    id
    seedLogin
    status
    expandedUsers
    discoveredUsers
    pendingUsers
    remainingRequests
    reserveRequests
    resetAt
    lastError
  }
}
```

`discoveryWarmup` reads only the current session owner's job and returns `null` before the first start:

```graphql
query DiscoveryWarmup {
  discoveryWarmup {
    id
    status
    currentLogin
    expandedUsers
    discoveredUsers
    pendingUsers
    frontierTruncated
    remainingRequests
    reserveRequests
    resetAt
    startedAt
    updatedAt
    completedAt
    lastError
  }
}
```

The status is `QUEUED`, `RUNNING`, `COMPLETED`, `RESERVE_PROTECTED`, or `FAILED`. `COMPLETED` means the stored frontier was exhausted. `RESERVE_PROTECTED` means either the post-batch observation reached 1,000 or the next expansion's conservative 13-request maximum could not fit above that floor; `remainingRequests` always exposes the actual count, so this status can honestly stop between 1,000 and 1,012. Status reads and startup recovery do not retry this terminal state. After `resetAt`, an explicit start preserves the same job and continues its frontier. `FAILED` retains a bounded error message and a later start creates a new attempt.

The worker performs one user expansion per batch and yields between batches. The local scheduler scans durable runnable jobs in limited batches, runs at most four concurrently, and refills a worker slot after each batch. A fresh shared neighborhood is reused only when all four collections have complete coverage; the read still uses the current app user so private saved projections remain private. Missing, stale, or incomplete neighborhoods go through GitHub. Cache-only batches do not spend or enforce a stale stored REST observation; the next cache miss performs the normal reserve preflight.

Following and follower logins extend a deduplicated breadth-first frontier. Expanded and pending logins together are capped at 10,000 users, well below the 190,000-node production import boundary; `frontierTruncated` reports dropped candidates, and the job completes after the retained frontier is exhausted. Every GitHub-backed public expansion still uses `github-user:<login>` deduplication, the authenticated account's fenced rate-budget lease, and the transactional shared-graph import. A separate `discovery-warmup:<app-user-id>` lease fences private progress writes. Neo4j stores the job on that user's `SyncState`; startup resumes `QUEUED` and `RUNNING` jobs, and a crash retries at most the uncommitted current frontier entry after lease expiry.

### Save a repository

```graphql
mutation SaveRepository(
  $fullName: String!
  $categories: [String!]!
  $note: String
) {
  saveRepository(
    fullName: $fullName
    categories: $categories
    note: $note
  ) {
    id
    fullName
    categories
    note
    createdAt
  }
}
```

The target repository must already exist in the shared imported graph. A save is private to the authenticated app user and is idempotent by repository target: saving the same repository again returns the existing bookmark rather than creating a duplicate or replacing its original categories/note. The mutation accepts categories and an optional note on the first save.

On Neo4j, bookmark creation and relationship writes share one transaction. A composite uniqueness constraint on `(user_id, target_kind, target_github_id)` backs the idempotent owner/target identity.

The `saved` value on each repository candidate is calculated against the current app user's bookmark overlay. It is not a public property on the repository and cannot leak another user's saves.

## Explainable repository ranking

Discovery starts with repositories connected to immediate followers or followed accounts and removes repositories already owned or starred by the seed user. Signals are accumulated per distinct nearby login, then scored in Rust so the file and Neo4j adapters use the same ranking function.

| Signal | Score contribution |
| --- | ---: |
| Distinct nearby recommender | `4.0` each |
| Recommender followed by the seed | `2.0` each |
| Recommender who follows the seed | `1.0` each |
| Nearby recommender starred the repository | `1.5` each |
| Nearby recommender owns the repository | `1.0` each |
| Global stars | `log10(stars + 1) x 1.25` |
| Fork adoption | `log10(forks + 1) x 0.75` |
| Language already present in the seed's repositories | `2.5` |
| Activity within 30 / 180 / 365 days | `3.0` / `1.75` / `0.75` |
| At most 5,000 stars and at least two recommenders | `2.0` hidden-gem bonus |
| Archived repository | `-8.0` |

Scores are rounded to two decimals. Results sort by score descending, then global stars descending, then full name ascending. This is not a simple "fewest stars first" list: nearby independent endorsements dominate, while reach, adoption, language affinity, recency, and archive state refine the result.

The GraphQL projection makes the ranking inspectable:

- `networkStars` is the count of nearby recommenders who starred the repository.
- `viaLogins` identifies the nearby accounts that connected the repository to the seed.
- `discoveryScore` is the calculated score.
- `reasons` contains human-readable contributing signals.
- repository metadata exposes stars, forks, language, activity, fork state, and archive state.

Repositories directly starred by the seed start at `18`; repositories owned by the seed start at `12`. They receive `+4` when they have at most 5,000 stars, `+2` when active within 180 days, and `-8` when archived before being merged into the final ordered result.

## Frontend workflow

### `/app/explore`

The entry route:

1. pre-fills the connected GitHub account
2. accepts a login, `@handle`, or `github.com/<login>` URL
3. links directly to the connected account's graph
4. offers up to eight privately bookmarked people as restart points

Opening a valid login creates `/app/explore/<login>?trail=<login>`.

### `/app/explore/:login`

The node route:

1. reads `neighborhood` from the shared cache
2. calls `expandUser` once when the user is not yet in the graph or its own neighborhood has never been fetched
3. keeps cached results visible during an explicit refresh
4. shows which collections are partial when GraphQL coverage is incomplete
5. renders direction-preserving Followers and Following lanes
6. appends each clicked login to the URL `trail`, bounded to the eight most recent entries
7. lets every breadcrumb return to the corresponding trail prefix
8. renders ranked repository cards with score, reasons, path accounts, metadata, and private save state
9. calls `saveRepository` and updates the local query cache after a successful save

The trail is URL state, not persisted private click history. It is therefore refreshable and shareable, while durable click-history storage remains future work.

### Saved and Settings

The authenticated React shell has three primary areas:

- Explore owns follower/following traversal and repository discovery.
- Saved owns bookmark, collection, and exploration-history views.
- Settings owns synchronization, request-budget visibility, and account controls.

Repository detail remains a contextual deep link rather than a fourth primary destination. The router preserves old entry points with these redirects:

| Former URL | React destination |
| --- | --- |
| `/app/bookmarks` | `/app/saved?view=bookmarks` |
| `/app/categories` | `/app/saved?view=collections` |
| `/app/explore/snapshots` | `/app/saved?view=history` |
| `/app/sync` | `/app/settings` |

## Strawn contract

`apps/web/package.json` and `ribbon.json` select the released Strawn package version and canonical source commit used by GitExplore.

The React app imports components and semantic tokens only from the `strawn` root entrypoint and icons only from the `strawn-icons` root entrypoint. Product-specific graph lanes, expedition trails, discovery cards, and application-shell composition remain in `apps/web`; GitExplore does not copy Strawn tokens or maintain a local design-system adapter.

When the design-system selection changes, update the package pin and manifest commit together. Do not deep-import Strawn internals or invent product-local replacements for an exported semantic token.

## Legacy REST compatibility

GraphQL is additive. These existing routes remain active for the earlier dashboard, CLI-aligned workflows, and typed client:

| Route | Purpose | Session requirement |
| --- | --- | --- |
| `GET /health` | process health | public |
| `GET /auth/status` | current connection status | anonymous-safe; uses cookie when present |
| `GET /auth/oauth/start` | start browser OAuth | public entry; may reuse an existing session |
| `GET /auth/oauth/callback` | complete OAuth and set the session cookie | OAuth callback |
| `POST /auth/logout` | invalidate the current server session and expire its cookie | current cookie when present |
| `POST /sync/run` | import the connected account's graph | required |
| `GET /sync/status` | read private sync state | required |
| `GET`, `POST /bookmarks` | list or create private bookmarks | required |
| `GET`, `POST /categories` | list or create private categories | required |
| `POST /explore` | create the legacy seed-based exploration snapshot | required |
| `GET /explore/snapshots` | list private legacy snapshots | required |

The REST private routes also resolve identity from `gitexplore_session`; client-controlled `user_id` query parameters are not supported.

## Current runtime boundary

Implemented now:

- synchronous public-profile expansion
- collection-bounded expansion with a 300-entry cap per followers, following, starred, and owned list
- GraphQL coverage flags and stale, non-destructive merging for capped partial collections
- conservative incomplete/stale treatment for legacy caches missing coverage
- public-only repository imports
- stable-id identity with normalized, rename-safe user and repository aliases
- direction-preserving shared graph import
- transactional Neo4j replacement of complete collections and merging of partial collections
- file and Neo4j neighborhood reads and ranking
- transactional, constraint-backed per-user private saves on Neo4j
- cookie-authenticated GraphQL
- bounded one-time OAuth state/nonce validation, durable bounded sessions, and canonical same-account reconnects
- React click trail, compatibility routing, and repository save workflow
- durable per-GitHub-identity REST budget status and fenced crawl serialization with a strict 1,000-request reserve
- durable, resumable, per-user discovery warmup with bounded batches and authenticated GraphQL status

Not implemented:

- general-purpose queued GitHub rate-budget scheduling beyond the synchronous reserve gate
- persisted private click history

Automated Rust and frontend checks exercise the in-process contracts. A live Neo4j, browser OAuth, and GitHub API round trip still requires Docker Desktop (or another Docker engine) plus valid GitHub OAuth credentials.
