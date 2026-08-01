# First-value onboarding

GitExplore presents a versioned, non-blocking guide to every authenticated account that has no terminal record for the current onboarding version. Version 1 teaches the product through real data: open a GitHub profile, follow one valid graph connection, and save one repository to the private overlay.

The guide is rendered inside the authenticated app shell, so OAuth return paths and direct links remain intact. It can be collapsed, dismissed, or replayed from Settings. Completion and dismissal persist with the canonical app user and therefore survive browser changes and GitHub reconnection.

## Progress rules

The server does not accept client-authored step flags. For the current onboarding `started_at` window it derives:

- `openedTrailhead` from a successfully recorded person visit
- `followedConnection` from a recorded trail containing at least two people
- `savedRepository` from a repository bookmark created after the window began
- `mappingStarted` from the presence of a private discovery-warmup job; this is informational and never required

Completing onboarding validates the three required facts again. Replay creates a new start timestamp, so actions from an older run do not complete the new run.

## Persistence

File mode stores a serde-defaulted per-user onboarding map in `graph.json`. Neo4j stores version, status, and lifecycle timestamps on the private `LocalUser` node. Missing or older-version state resolves to `NOT_STARTED`; no client operation accepts an app-user id.

## Browser behavior

The app begins lifecycle tracking when the version-one guide first appears. It does not start discovery mapping automatically. The optional mapping action explains the protected 1,000-request reserve and invokes the existing idempotent warmup mutation only after explicit user input. Mapping failures and reserve-protected states do not block the three-step activation path.

The UI invalidates onboarding progress after successful visit and repository-save operations. Once all required facts are present, it completes the lifecycle and shows a session-level success message with links to Saved and continued exploration.

## GraphQL operations

All operations use the server-managed browser session:

```graphql
query OnboardingProgress {
  onboardingProgress {
    version
    status
    startedAt
    completedAt
    dismissedAt
    openedTrailhead
    followedConnection
    savedRepository
    mappingStarted
  }
}
```

The mutations are `beginOnboarding`, `dismissOnboarding`, `restartOnboarding`, and `completeOnboarding`. Each returns the same progress object. Begin and complete are idempotent; complete returns `BAD_USER_INPUT` until all required actions have succeeded.
