# Orphaned Session Migration

Canonical root: `C:\Users\loqpm\Desktop\Paul\products\prototypes\rust\git_explore`

Previous root: `C:\Users\loqpm\Desktop\Rust\GitExplore`

## Durable decisions recovered

- Shared public graph data is cached once for all users; private user overlays remain isolated.
- Serve cached data immediately and expose freshness while one entity-keyed refresh runs in the background.
- GitHub rate limits protect GitHub; application queues, deduplication, and quotas must protect GitExplore's workers, database, and shared cache.
- Browser APIs use cookie-based sessions rather than a client-selected user identifier.
- The Rust backend and Svelte workspace are one product and should be verified together.

## Source tasks

- `019f2bb5-33fd-7c22-b9ba-bef718b6d093` — architecture planning and implementation of the shared-graph/private-overlay foundation.
- `019f3106-47e7-71a1-9d33-b3f1081a0303` — code explanation and onboarding context.

The implementation was previously verified with `cargo test`, the API-client tests, and the Svelte check. Run current verification again after migration before relying on that historical result.
