# Production deployment

GitExplore targets one Vercel Services project at `gitexplore.moriatz.com`. The React/Vite frontend and Rust/Axum API deploy atomically and share one browser origin. Neo4j remains an external managed graph database.

This document describes the deployment scaffold. It does not claim that the production URL is live. `ribbon.json` remains `status: local` until the live checks pass. Its separate `productionReadiness` field is a fail-closed declaration that provider, secret, schema, build, and release prerequisites are ready for a production deployment attempt.

## Service topology

`vercel.json` declares two services:

- `web` builds the React/Vite single-page application. Its service-local rewrite checks the real filesystem first and then falls back to `/index.html`, so direct `/login` and `/app/*` requests reach React without turning JavaScript or CSS requests into HTML.
- `api` builds `Dockerfile.vercel` as a Rust container and receives `/auth/*`, `/graphql`, `/health`, `/sync/*`, `/bookmarks`, `/categories`, and `/explore*`.
- The final top-level catch-all selects `web` without changing the request path. `services.web.rewrites` owns the Services-mode SPA fallback; `apps/web/vercel.json` keeps the same fallback when the Vite app is deployed standalone.

The linked Vercel project's Framework Preset must be set to **Services**. Vercel only activates this topology when that project setting and the top-level `services` configuration are both present. Automatic Vercel Git deployments are disabled in `vercel.json`; the protected, main-only release workflow owns production builds and domain updates.

The browser uses same-origin API paths; Vercel routes those paths to the API service before sending other requests to the web service. The GitHub OAuth callback must be registered exactly as:

```text
https://gitexplore.moriatz.com/auth/oauth/callback
```

Vercel supplies `PORT` to the Rust container. `AppConfig` binds `0.0.0.0:<PORT>` when `GITEXPLORE_SERVER_ADDR` is not explicitly set, preserving the existing local override.

`Dockerfile.vercel` also fixes `GITEXPLORE_DEPLOYMENT_MODE=production` and `GITEXPLORE_GRAPH_BACKEND=neo4j`. Production startup fails closed unless the frontend origin is HTTPS, the exact OAuth callback and credentials exist, the Neo4j URI is encrypted, identity encryption is configured, and both database capacity limits exist. The static web bundle contains no API host; browser requests remain on the incoming origin.

The protected release workflow builds the Rust release binary once on the pinned
`ubuntu-22.04` runner, uses that same binary for the Aura schema gate, and hands
it to `Dockerfile.vercel`. The Docker build executes the binary in both its
Bookworm build and runtime stages before deployment, so an ABI mismatch fails
before the artifact can ship. When the handoff file is absent, local
`docker build` commands keep using the self-contained Rust builder path.

## Fail-closed production blockers

The runtime no longer depends on a writable container filesystem: Neo4j stores encrypted GitHub connections, pending OAuth state, sessions, graph data, fenced entity refresh leases, and per-GitHub-identity REST budget state/leases. The budget properties live on the already-constrained `GitHubIdentity` node, so the immutable version-1 schema migration does not change. The embedded schema has an idempotent release migration and read-only startup check. The Aura instance, rotated credentials, Vercel/GitHub secret configuration, GitHub OAuth application, scoped DNS record, build, and local verification gates are provisioned and passing.

The entries in `ribbon.json.productionReadiness.blockers` are authoritative for whether a deployment may be attempted. Remove a blocker only after that prerequisite exists and is verified. Set `ready` to `true` only when the list is empty; otherwise `.github/workflows/release.yml` exits before accessing provider credentials or building a production artifact. This field does not claim the hostname has passed live verification: `status` stays `local` until the post-deploy login, cross-instance session, exploration, bookmark, rate-limit, cache, DNS, TLS, and security-header checks pass.

The temporary `GITEXPLORE_DATA_DIR=/tmp/gitexplore` in `Dockerfile.vercel` is used only for local compatibility paths. Production identity and graph state must remain in Neo4j.

## Provider configuration

Keep all values in provider secret stores. Do not commit `.env`, `.vercel`, credentials, IDs, or generated deployment output.

GitHub Actions `production` environment secrets (there are no repository-wide copies):

- `VERCEL_TOKEN`
- `VERCEL_ORG_ID`
- `VERCEL_PROJECT_ID`
- `GITEXPLORE_IDENTITY_ENCRYPTION_KEY`
- `GITEXPLORE_NEO4J_URI`
- `GITEXPLORE_NEO4J_USERNAME`
- `GITEXPLORE_NEO4J_PASSWORD`

GitHub Actions production variable:

- `GITEXPLORE_NEO4J_DATABASE` (defaults to `neo4j`)

The GitHub `production` environment accepts deployments only from the `main` branch. The crawl workflow also checks out `main` explicitly and exposes database secrets only to its two Rust execution steps.

Vercel server-only runtime values:

- `GITEXPLORE_FRONTEND_ORIGIN`
- `GITEXPLORE_DEPLOYMENT_MODE=production` (also fixed in the production image)
- `GITEXPLORE_GITHUB_CLIENT_ID`
- `GITEXPLORE_GITHUB_CLIENT_SECRET`
- `GITEXPLORE_GITHUB_REDIRECT_URI`
- `GITEXPLORE_GITHUB_SCOPES`
- `GITEXPLORE_GRAPH_BACKEND=neo4j`
- `GITEXPLORE_IDENTITY_ENCRYPTION_KEY`
- `GITEXPLORE_NEO4J_URI`
- `GITEXPLORE_NEO4J_USERNAME`
- `GITEXPLORE_NEO4J_PASSWORD`
- `GITEXPLORE_NEO4J_DATABASE`
- `GITEXPLORE_NEO4J_MAX_TOTAL_NODES=190000`
- `GITEXPLORE_NEO4J_MAX_TOTAL_RELATIONSHIPS=380000`

The browser bundle requires no API-origin environment variable. Keep server credentials out of the web service; same-origin routing is part of the checked deployment contract.

Hostinger DNS remains local to Ribbon. Import the DNS credential through Ribbon's secure credential path, then provision only the manifest hostname. `ribbon.json` records the project-specific, Vercel-inspected TTL-300 CNAME target for `gitexplore.moriatz.com`; Ribbon must stop rather than guess or replace a conflicting record type.

### Aura Free launch limits

The launch database is AuraDB Free. The Aura console currently caps it at 200,000 nodes and 400,000 relationships, provides limited backups, and may delete an instance after 30 days of inactivity. GitExplore's production graph-import boundary is configured to stop conservatively at 190,000 total database nodes or 380,000 total relationships. Every admission check counts existing identity, session, lease, and private-overlay data—not merely nodes observed by one crawl—and the remaining 10,000-node/20,000-relationship headroom is reserved for ancillary writes that do not pass through the graph-import mutex. This is an import safety gate, not a replacement for provider monitoring. Treat Aura Free as a launch environment rather than a production durability guarantee: export recoverable graph data regularly, and upgrade the instance before relying on stronger backup or availability commitments.

## Verification before release

Run from the repository root:

```powershell
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
pnpm install --frozen-lockfile
pnpm --filter @gitexplore/api-client test
pnpm --filter @gitexplore/web check
pnpm --filter @gitexplore/web test
pnpm check
pnpm production:check
pnpm --filter @gitexplore/web build
docker build --file Dockerfile.vercel --tag gitexplore-vercel-verify .
```

### Windows operator path for Aura schema commands

Run Aura migration commands from the Linux application image rather than from a
Windows-hosted Rust binary. `neo4rs` 0.8 reads the host's native certificate
store, and some Windows trust-store configurations reject Aura's otherwise valid
public chain with `UnknownIssuer`. The production image installs Debian's
`ca-certificates` bundle and keeps hostname and certificate verification enabled.

Put only the required server values in an ignored `.env.aura` file:

```dotenv
GITEXPLORE_GRAPH_BACKEND=neo4j
GITEXPLORE_IDENTITY_ENCRYPTION_KEY=<32-byte-unpadded-base64url-key>
GITEXPLORE_NEO4J_URI=neo4j+s://<instance-id>.databases.neo4j.io
GITEXPLORE_NEO4J_USERNAME=<username>
GITEXPLORE_NEO4J_PASSWORD=<password>
GITEXPLORE_NEO4J_DATABASE=<instance-specific-database-name>
```

Then build once and run both gates from PowerShell:

```powershell
docker build --file Dockerfile.vercel --tag gitexplore-schema-verify .
docker run --rm --env-file .env.aura gitexplore-schema-verify gitexplore --format json schema apply
docker run --rm --env-file .env.aura gitexplore-schema-verify gitexplore --format json schema check
```

Do not replace `neo4j+s://` with an unencrypted scheme or disable certificate
verification. Aura Free database names can be instance-specific, so use the name
reported by Aura rather than assuming `neo4j`.

Run Ribbon from its control-plane checkout:

```powershell
pnpm ribbon check C:\path\to\git_explore
```

## Release and live verification

Production is released only by a reviewed change merged to `main`:

1. Preserve the existing remote repository history and create a `feature/*` branch.
2. Pass local checks and the extended Ribbon check.
3. Provision or verify the Hostinger CNAME through Ribbon.
4. Open a pull request and wait for required checks and review.
5. Merge to `main`; do not run a local production deployment.
6. The release workflow restores exact-version caches, verifies Rust, browser, and production contracts, applies and checks the embedded Aura schema migration, uses the lockfile-pinned Vercel CLI, pulls production settings, builds once, and deploys with `--prebuilt --prod`.
7. After DNS, TLS, API, and security-header probes pass, pinned Chromium boots the canonical `/login` and protected `/app/explore` routes at a 375px viewport. The smoke test fails on missing scripts/styles, browser errors, broken SPA rewrites, malformed OAuth return paths, horizontal overflow, or collapsed display typography.
8. The workflow waits for Vercel readiness, assigns the custom domain, then verifies canonical TLS, Neo4j-backed `/health`, unauthenticated auth shape, the frontend, and security headers before uploading `deployment.json`.
9. Exercise GitHub login, session persistence across a rerun of the same release, GraphQL exploration, repository bookmarks, rate-limit visibility, and Neo4j cache reads.
10. Inspect browser errors and Vercel logs before recording the production URL and changing the Ribbon status to `production`.

The Aura instance and GitHub OAuth application are provisioned, with rotated server credentials installed in Vercel Production and GitHub Actions. The embedded migration was applied and checked successfully against Aura from the Linux production image. Production prerequisites are ready, but the deployment, replacement-instance session check, DNS/TLS result, and complete live product verification still need to finish before `ribbon.json.status` can change to `production`.

## References

- [Runtime and local Compose flow](runtime.md)
- [Authentication flow](ui-login-flow.md)
- [Graph architecture](graph-explorer.md)
- [Vercel Services](https://vercel.com/docs/services)
- [Vite on Vercel](https://vercel.com/docs/frameworks/frontend/vite)
- [Docker on Vercel](https://vercel.com/blog/dockerfile-on-vercel)
