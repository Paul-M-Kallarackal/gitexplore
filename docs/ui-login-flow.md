# UI Login and Session Flow

This document is the source of truth for browser authentication between the React/Vite frontend and Rust backend.

## Implemented boundary

- React Router owns `/login` and the authenticated `/app` routes in the browser.
- Rust owns GitHub OAuth URL generation, one-time state and browser-nonce validation, callback exchange, canonical app-user identity resolution, and session creation.
- The browser stores only the opaque `gitexplore_session` cookie. GitHub access tokens stay in the backend identity store.
- Pending OAuth state is durable through the active identity repository, valid for 10 minutes, and bounded to 256 entries.
- Server session records expire after 30 days, are purged as they expire, and are bounded to 4,096 active records.
- Private GraphQL and REST operations resolve the app user from that cookie. They do not accept `user_id` from the browser.

## Flow

### 1. Load `/login`

The React application calls `GET /auth/status` through `packages/api_client` before resolving protected routes.

- If the cookie resolves to a connected account, `/login` redirects to `/app`.
- Otherwise the login page renders one GitHub connection action.

### 2. Start OAuth at Rust

The page builds the URL with:

```ts
api.startBrowserOAuth(`${appOrigin}/app`)
```

That produces:

```text
GET /auth/oauth/start?redirect_to=http://localhost:3000/app
```

There is no client-supplied OAuth `state` or `user_id` query parameter. The backend:

1. generates opaque state and a browser nonce
2. stores the pending app-user/nonce/redirect context server-side for at most 10 minutes; the identity repository keeps at most 256 pending states and evicts the oldest when full
3. sets `gitexplore_oauth_nonce=<opaque-id>; Path=/auth/oauth; HttpOnly; SameSite=Lax; Max-Age=600`
4. restricts the requested redirect to `GITEXPLORE_FRONTEND_ORIGIN`
5. redirects to GitHub with the server-generated state

### 3. Complete the callback

GitHub returns the browser to:

```text
GET /auth/oauth/callback?code=...&state=...
```

Rust:

1. requires the path-scoped nonce cookie
2. looks up the opaque state, validates the matching nonce and 10-minute lifetime, and consumes the state once
3. exchanges the GitHub code using the configured client id, client secret, and callback URI; the default requested scope is `read:user`
4. canonicalizes the connection by GitHub's stable user id
5. creates an opaque session id
6. sets `gitexplore_session=<opaque-id>; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000`
7. clears `gitexplore_oauth_nonce`
8. redirects to the validated frontend path with `?connected=1`

The nonce and session cookies add `Secure` when `GITEXPLORE_FRONTEND_ORIGIN` uses HTTPS. The completed server-side session and pending OAuth state are persisted through the active identity repository. On Neo4j, any healthy replica can complete the callback or resolve the 30-day session. Reconnecting the same GitHub account reuses its canonical app-user id, preserving its bookmarks, categories, snapshots, and sync state. Connecting a different account does not overwrite that private overlay.

For the default login flow the browser lands on:

```text
http://localhost:3000/app?connected=1
```

### 4. Protect `/app`

The React auth boundary waits for the status query before resolving `/app`. If the account is not connected it redirects to `/login`; otherwise it renders the application shell and directs the index route to the Explore trailhead, prefilled with the connected account. A backend failure remains an explicit retryable error and is not treated as a logged-out response.

Browser requests target the page origin and use `credentials: "include"`. During development, Vite proxies API paths to the Rust service; Docker Compose sets the server-only proxy target to `http://gitexplore:4000`. In production, Vercel routes those same paths to the Rust service under the single GitExplore origin. All frontend data loading happens in the browser, with no browser-exposed API-origin setting.

### 5. Sign out

The application shell calls:

```http
POST /auth/logout
```

Rust removes the presented session id from the server-side identity store and returns `gitexplore_session` with `Max-Age=0`. Reusing the old cookie is therefore unauthorized. This browser sign-out does not remove the stored GitHub credential or the stable account link.

A credential disconnect removes the connection and access token but retains the GitHub-id-to-app-user link. Reconnecting that GitHub account later restores the same canonical private overlay.

### 6. Enter the explorer

`/app/explore` uses the connected GitHub login as the default starting point. `/app/explore/:login` calls the cookie-authenticated GraphQL operations described in [Graph explorer and GraphQL](graph-explorer.md).

The authenticated shell has three primary areas: Explore, Saved, and Settings. Saved combines bookmarks, collections, and exploration history. Compatibility redirects map `/app/bookmarks`, `/app/categories`, and `/app/explore/snapshots` into the corresponding Saved view, and `/app/sync` into Settings.

## Relevant files

- `apps/web/src/`: React routes, authentication boundary, query state, and product composition
- `apps/web/vite.config.ts`: development API proxy and browser-test configuration
- `vercel.json`: production Services routing, including direct `/login` and `/app/*` request-path transforms to the SPA entry point
- `apps/web/vercel.json`: standalone Vite SPA fallback
- `packages/api_client/src/index.ts`: credentialed REST/GraphQL client and OAuth URL helper
- `src/http.rs`: OAuth routes, cookie creation/resolution, CORS, GraphQL routing
- `src/identity.rs` and `src/application.rs`: connection and session behavior
- `src/config.rs`: frontend origin and GitHub OAuth configuration

## Required environment

Frontend development server (optional):

- `GITEXPLORE_DEV_API_BASE_URL` (defaults to `http://127.0.0.1:4000`)

Backend:

- `GITEXPLORE_GITHUB_CLIENT_ID`
- `GITEXPLORE_GITHUB_CLIENT_SECRET`
- `GITEXPLORE_GITHUB_REDIRECT_URI`
- `GITEXPLORE_GITHUB_SCOPES`
- `GITEXPLORE_FRONTEND_ORIGIN`
- `GITEXPLORE_DATA_DIR`

Default local values:

```dotenv
GITEXPLORE_GITHUB_REDIRECT_URI=http://localhost:4000/auth/oauth/callback
GITEXPLORE_GITHUB_SCOPES=read:user
GITEXPLORE_FRONTEND_ORIGIN=http://localhost:3000
GITEXPLORE_DATA_DIR=.gitexplore-data
```

The callback URI must match the GitHub OAuth application registration exactly. Compose overrides the Vite proxy target with the Rust service URL inside its network. Production requires no client-side API base because the browser and API share one origin.

## UI contract

The login and app surfaces consume components, semantic tokens, and icons directly from the public root entrypoints of `strawn` and `strawn-icons`. Product-specific route composition stays in `apps/web`; GitExplore does not maintain a parallel component library or token adapter. See [Graph explorer and GraphQL](graph-explorer.md#strawn-contract).

## Security and scope notes

- The session cookie is `HttpOnly` and `SameSite=Lax`; frontend JavaScript does not need to read it.
- Completed sessions are durable; cookies and server records expire after 30 days. Neo4j stores keyed session-id digests rather than cookie values. Expired records are purged during create/resolve operations, and the identity store retains at most 4,096 active sessions.
- Browser logout clears both the current server mapping and cookie; stable account links survive credential disconnects.
- OAuth state is opaque, one-time, expires after 10 minutes, and is bound to the initiating browser with an `HttpOnly`, `SameSite=Lax` nonce cookie. It is durable, capped at 256 pending entries, and survives instance replacement when Neo4j is active.
- The nonce and session cookies add `Secure` when the configured frontend origin uses HTTPS.
- Redirects are restricted to `GITEXPLORE_FRONTEND_ORIGIN`, which is also the credentialed browser/CORS origin.
- Stable GitHub user ids canonicalize reconnects so the same account retains one private overlay.
- The default OAuth scope is `read:user`, not `repo`; graph imports retain only public repositories.
- Identity and graph JSON updates use a flushed, fsynced same-directory temporary file before replacing the destination. `identity.json` stores GitHub access tokens and pending browser nonces only as authenticated ciphertext and requires `GITEXPLORE_IDENTITY_ENCRYPTION_KEY`.
- GitHub and OAuth HTTP clients use a 10-second connection timeout and a 30-second request timeout.
- OAuth credentials must stay in environment files or deployment secret storage.

## Live verification

1. Start the stack with `docker compose up --build`.
2. Open `http://localhost:3000/login`.
3. Select the GitHub connection action.
4. Confirm the browser passes through `http://localhost:4000/auth/oauth/start`.
5. Complete GitHub OAuth.
6. Confirm the callback sets `gitexplore_session` and returns to `/app`.
7. Open `/app/explore` and expand a graph node.

This end-to-end check requires Docker Desktop (or another Docker engine), a real GitHub OAuth application, and valid credentials. Automated repository tests do not replace it.
