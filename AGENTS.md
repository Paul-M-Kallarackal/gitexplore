# GitExplore Project Instructions

## Architecture

GitExplore imports a GitHub social and repository graph through a Rust service and exposes CLI, HTTP, and React product surfaces.

Preserve the shared-cache/private-overlay model:

- Public GitHub users, repositories, and relationship facts belong to the shared graph.
- Bookmarks, categories, exploration snapshots, sync state, and future click history remain private per authenticated app user.
- Browser routes resolve the user from the server-managed `gitexplore_session` cookie; do not restore client-controlled `user_id` parameters.
- Cached reads should remain available while stale data refreshes.
- Refresh work should deduplicate by entity key, such as `github-user:<login>` or `github-repo:<owner/name>`.

The current repository implements the shared graph, private overlay, cookie session, and freshness response foundation. Background workers, queueing/rate limiting, public-profile expansion jobs, and private click history remain future work unless current code proves otherwise.

## Repository boundaries

- `src/ports.rs` defines contracts.
- `src/application.rs` owns service behavior.
- `src/adapters.rs` implements file and Neo4j persistence.
- `src/http.rs` owns the Axum HTTP/session surface.
- `packages/api_client/` owns the typed browser client.
- `apps/web/` owns the React/Vite UI and consumes public APIs from `strawn` and `strawn-icons`.
- `docs/` is the architecture and runtime source of truth.

## Verification

```powershell
cargo test
pnpm --filter @gitexplore/api-client test
pnpm --filter @gitexplore/web check
pnpm check
```

Use `docker compose up --build` for the full local stack. Keep secrets in environment files and never commit GitHub OAuth credentials.

The canonical project root is `C:\Users\loqpm\Desktop\Paul\products\prototypes\rust\git_explore`. Do not write new work into `C:\Users\loqpm\Desktop\Rust\GitExplore`.
