# GitExplore web

The browser product is a React 19 single-page application built with Vite. It uses the typed `@gitexplore/api-client` and the public root exports from `strawn` and `strawn-icons`.

## Developing

Install the workspace from the repository root, then start the web app:

```bash
pnpm install
pnpm --filter @gitexplore/web dev
```

The app sends same-origin, credentialed requests. Vite proxies API paths to `http://127.0.0.1:4000` by default. To use another local API origin, configure the Vite server rather than exposing an API base URL to browser code:

```powershell
$env:GITEXPLORE_DEV_API_BASE_URL='http://localhost:4000'
pnpm --filter @gitexplore/web dev
```

Docker Compose supplies `http://gitexplore:4000` as that proxy target inside its network.

## Product routes

- `/login`: GitHub connection and session recovery
- `/app/explore/:login?`: follower/following traversal and repository discovery
- `/app/saved`: bookmarks, collections, and exploration history
- `/app/settings`: sync, GitHub request budget, and account controls
- `/app/repository/:owner/:repo`: deep-linkable repository detail

The router redirects the former bookmarks, categories, snapshots, and sync URLs into the matching Saved or Settings view.

Authenticated routes share a versioned first-value onboarding provider. The non-blocking guide tracks real profile visits, valid connection trails, and repository saves through cookie-authenticated GraphQL state. It can be skipped or replayed from Settings, and its optional discovery mapping action never runs without an explicit click.

## Verify and build

```bash
pnpm --filter @gitexplore/web check
pnpm --filter @gitexplore/web test
pnpm --filter @gitexplore/web build
```

Preview the built SPA with `pnpm --filter @gitexplore/web preview`.

Production is released with the Rust service through the reviewed repository workflow; do not deploy this directory independently.
