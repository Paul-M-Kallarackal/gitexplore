# GitExplore

GitExplore is a Rust, Neo4j, and React application for walking GitHub follower/following graphs and finding repositories with strong nearby signal before they become obvious. Public GitHub facts are shared in the graph; saves, categories, snapshots, sessions, and sync state remain private to the authenticated app user.

The repository now also contains a `pnpm + Turborepo` UI workspace:

- `apps/web`: React/Vite product UI
- `packages/api_client`: typed HTTP client for the Rust API

The web app consumes the canonical public `strawn` and `strawn-icons` package entrypoints. Product-specific graph, repository, and application-shell composition stays in `apps/web`; there is no product-local design-system package.

The current graph-explorer slice provides:

- `POST /graphql`, authenticated by the server-managed `gitexplore_session` cookie
- directional follower and following expansion for any public GitHub login, capped at 300 entries per imported collection with explicit coverage
- explainable repository ranking based on nearby people, stars, ownership, language, activity, global reach, and archive state
- public-only repository imports and private, idempotent repository saves
- coverage-aware transactional Neo4j graph updates and constraint-backed bookmark uniqueness
- stable numeric GitHub identity with normalized, rename-safe user and repository aliases
- refreshable, shareable click trails at `/app/explore/:login?trail=...`

The primary application areas are Explore, Saved, and Settings. Saved combines bookmarks, collections, and exploration history. Compatibility redirects preserve the former `/app/bookmarks`, `/app/categories`, `/app/explore/snapshots`, and `/app/sync` URLs.

See [Graph explorer and GraphQL](docs/graph-explorer.md) for the API operations, ranking contract, and frontend workflow.

## Quick start

1. Copy `.env.example` to `.env`.
2. Fill in `GITEXPLORE_GITHUB_CLIENT_ID`, `GITEXPLORE_GITHUB_CLIENT_SECRET`, and a unique `GITEXPLORE_IDENTITY_ENCRYPTION_KEY` (generation is documented below). Compose has a public development-only fallback so an empty local value remains usable; never use that fallback outside local development.
3. Start the local stack:

```bash
docker compose up --build
```

Docker Desktop or another working Docker engine must be running before this command will succeed.

4. Open the product surfaces:

```bash
http://localhost:3000
http://localhost:4000/health
```

5. Open `http://localhost:3000/login` and choose **Continue with GitHub**.
6. After GitHub returns you to GitExplore, open `http://localhost:3000/app/explore` and start an expedition.

Browser OAuth uses opaque, one-time server-side state that expires after 10 minutes plus an `HttpOnly`, `SameSite=Lax` browser nonce. With the Neo4j backend, OAuth state, account links, encrypted credentials, and sessions are durable across service replicas. A successful callback creates an opaque app-user session and sets `gitexplore_session`; GraphQL and private REST requests do not accept a client-selected `user_id`. Session records and their cookies expire after 30 days. The server purges expired sessions and bounds the identity store to 4,096 active sessions. `POST /auth/logout` removes the current server-side session mapping and expires the browser cookie.

The CLI also supports GitHub device flow when **Enable Device Flow** is explicitly turned on for the OAuth app:

```bash
docker compose exec gitexplore gitexplore auth login --format json
docker compose exec gitexplore gitexplore sync run --format json
```

Reconnecting the same stable GitHub account reuses its canonical app-user identity, so its bookmarks, categories, snapshots, and sync state stay attached. Stable account links survive credential disconnects for that reason. The default GitHub scope is `read:user`, and repository imports retain only public repositories.

Each expansion reports whether followers, following, starred repositories, and owned repositories were fetched completely. A collection that reaches the 300-entry cap while GitHub still has another page is partial: GitExplore merges those entries with earlier cached edges and keeps the neighborhood `STALE`. Only collections with complete coverage replace their prior edges, within the same Neo4j transaction. Legacy cached users with no coverage fields are treated as incomplete and stale until refreshed.

## Populate the shared graph safely

An operator can breadth-first crawl following and follower neighborhoods while preserving at least 1,000 requests in the connected GitHub account's current REST `core` window:

```bash
gitexplore --user-id <server-derived-app-user-id> --format json graph crawl \
  --login <seed-login> \
  --request-reserve 1000 \
  --max-expansions 350 \
  --max-discovered-nodes 180000
```

The canonical app-user id is returned only by authenticated `GET /auth/status`. Production operators should dispatch the **Populate graph** GitHub Actions workflow instead of handling database or OAuth secrets locally. The crawler follows fresh cached neighborhoods without spending requests, expands following before followers, deduplicates its frontier, and stops at the reserve, the expansion cap, the node safety cap, the Neo4j import-capacity boundary, or an exhausted frontier. `--request-reserve` may be raised above 1,000; the selected value is enforced by a live durable preflight before each expansion, not only by the crawler's cached status. Rerunning is safe because imported public facts are shared and cache-aware.

Authenticated OAuth requests normally use a 5,000-request-per-hour window—not a daily or lifetime allocation. GitHub's returned `limit`, `remaining`, and `reset` values are authoritative. GitExplore's durable cross-replica gate refuses a refresh whose conservative maximum cost would cross the 1,000-request floor. See GitHub's [REST API rate-limit documentation](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api).

Docker Desktop (or another working Docker engine) and valid GitHub OAuth credentials are required for live Neo4j/OAuth verification. GitHub API and OAuth clients use a 10-second connection timeout and a 30-second request timeout. Unit and type checks do not prove that external round trip.

## Local development without Docker

Install workspace dependencies once:

```bash
pnpm install
```

Run the React/Vite app against the API:

```bash
pnpm --filter @gitexplore/web dev
```

During local development, Vite proxies `/auth`, `/graphql`, `/health`, `/sync`, `/bookmarks`, `/categories`, and `/explore` to `http://127.0.0.1:4000` by default. Set the server-only `GITEXPLORE_DEV_API_BASE_URL` in the Vite process when the API is elsewhere. Docker Compose sets it to `http://gitexplore:4000`, while the browser continues to target its own `http://localhost:3000` origin. Production routes those same paths to the Rust service under the single deployed origin.

Run the Rust API locally with file storage:

```bash
$env:GITEXPLORE_GRAPH_BACKEND='file'
$env:GITEXPLORE_IDENTITY_ENCRYPTION_KEY='<32-byte-base64url-key>'
cargo run -- serve
```

The repository is pinned to Rust `1.91.0` in [rust-toolchain.toml](rust-toolchain.toml).

To target Neo4j directly:

```bash
$env:GITEXPLORE_GRAPH_BACKEND='neo4j'
$env:GITEXPLORE_NEO4J_URI='neo4j://localhost:7687'
$env:GITEXPLORE_NEO4J_USERNAME='neo4j'
$env:GITEXPLORE_NEO4J_PASSWORD='gitexplore-dev-password'
$env:GITEXPLORE_IDENTITY_ENCRYPTION_KEY='<32-byte-base64url-key>'
cargo run -- serve
```

The file backend remains available for local development and now encrypts access tokens and pending browser nonces in `identity.json`. With the Neo4j backend, identity state is stored in Neo4j instead of the container filesystem, so OAuth callbacks and sessions survive instance replacement and work across replicas. Public graph facts remain shared; identity/session nodes and existing private overlay nodes remain keyed to the canonical app-user id.

Generate the required master key as 32 cryptographically random bytes encoded as unpadded base64url. For PowerShell:

```powershell
$identityBytes = New-Object byte[] 32
$identityRng = [Security.Cryptography.RandomNumberGenerator]::Create()
$identityRng.GetBytes($identityBytes)
$identityRng.Dispose()
([Convert]::ToBase64String($identityBytes).TrimEnd('=')).Replace('+', '-').Replace('/', '_')
```

Store the result only in the server secret manager as `GITEXPLORE_IDENTITY_ENCRYPTION_KEY`. GitExplore derives separate encryption and opaque-id keys with HKDF-SHA-256. Secrets use `v1.<nonce>.<ciphertext-and-tag>` with XChaCha20-Poly1305; the nonce is 24 random bytes and both segments are unpadded base64url. GitHub account id or OAuth state is authenticated as associated data. Session and OAuth-state identifiers are stored in Neo4j only as keyed HMAC-SHA-256 digests.

To preserve an existing local login before switching a deployment from `identity.json` to Neo4j, stop old writers, set the Neo4j environment and the new encryption key, apply and verify the embedded schema, then run the identity migration exactly once:

```powershell
cargo run -- --format json schema apply
cargo run -- --format json schema check
cargo run -- identity migrate-to-neo4j --confirm
```

The guarded command first rewrites any legacy plaintext token and pending nonce in `identity.json` through the existing atomic file replacement, copies active connections, account links, pending OAuth attempts, and unexpired sessions into Neo4j, and writes a completion marker only after all copies succeed. A second run is refused. Keep the encrypted source as a recovery artifact until the migrated login has been verified, then archive it under the same access controls as any credential backup.

## Verification

```powershell
cargo test
pnpm --filter @gitexplore/api-client test
pnpm --filter @gitexplore/web check
pnpm check
```

## Docs

- [Docs index](docs/README.md)
- [Architecture](docs/architecture.md)
- [Data model](docs/data-model.md)
- [Graph explorer and GraphQL](docs/graph-explorer.md)
- [Docker runtime](docs/runtime.md)
- [UI login flow](docs/ui-login-flow.md)
