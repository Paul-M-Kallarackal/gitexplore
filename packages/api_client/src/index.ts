export type ConnectedAccount = {
  github_user_id: number;
  login: string;
  display_name: string | null;
};

export type ConnectionStatus = {
  authenticated: boolean;
  app_user_id: string | null;
  connected: boolean;
  account: ConnectedAccount | null;
};

export type SyncState =
  | 'NeverSynced'
  | 'SyncInProgress'
  | 'SyncSucceeded'
  | 'SyncFailed';

export type SyncStatus = {
  state: SyncState;
  last_synced_at: string | null;
  last_error: string | null;
};

export type SyncSummary = {
  followers: number;
  following: number;
  starred_repositories: number;
  repositories: number;
  synced_at: string;
  coverage: GraphCoverage;
};

export type BookmarkTarget =
  | { GitHubUser: { login: string } }
  | { GitHubRepository: { full_name: string } };

export type Bookmark = {
  id: string;
  target: BookmarkTarget;
  categories: string[];
  note: string | null;
  created_at: string;
};

export type Category = {
  name: string;
  description: string | null;
};

export type ExplorationSeed =
  | { User: { login: string } }
  | { Repository: { full_name: string } }
  | { Category: { name: string } };

export type ExplorationSnapshot = {
  id: string;
  seed: ExplorationSeed;
  discovered_people: string[];
  discovered_repositories: string[];
  generated_at: string;
};

export type ExplorationResult = {
  seed: ExplorationSeed;
  related_people: string[];
  related_repositories: string[];
  saved_snapshot: ExplorationSnapshot;
  cache_status: 'Fresh' | 'Stale' | 'Refreshing' | 'RefreshFailed' | 'fresh' | 'stale' | 'refreshing' | 'refresh_failed';
  last_fetched_at: string | null;
  refresh_job_status: 'Queued' | 'Running' | 'Failed' | null;
  overload_message: string | null;
};

export type GraphUser = {
  githubId: string;
  login: string;
  name: string | null;
  url: string;
  avatarUrl: string | null;
  bio: string | null;
  followersCount: number | null;
  followingCount: number | null;
};

export type GraphRepository = {
  githubId: string;
  ownerLogin: string;
  name: string;
  fullName: string;
  description: string | null;
  htmlUrl: string;
  stargazerCount: number;
  forkCount: number;
  primaryLanguage: string | null;
  topics: string[];
  updatedAt: string | null;
  archived: boolean;
  fork: boolean;
};

export type RepositoryCandidate = {
  repository: GraphRepository;
  networkStars: number;
  viaLogins: string[];
  discoveryScore: number;
  reasons: string[];
  saved: boolean;
};

export type GraphCoverage = {
  followersComplete: boolean;
  followingComplete: boolean;
  starredRepositoriesComplete: boolean;
  repositoriesComplete: boolean;
};

export type UserNeighborhood = {
  user: GraphUser;
  followers: GraphUser[];
  following: GraphUser[];
  repositories: RepositoryCandidate[];
  cacheStatus: 'FRESH' | 'STALE' | 'REFRESHING' | 'REFRESH_FAILED';
  lastFetchedAt: string | null;
  coverage: GraphCoverage;
};

export type SavedRepository = {
  id: string;
  fullName: string;
  categories: string[];
  note: string | null;
  createdAt: string;
};

export type GitHubRateLimit = {
  limit: number;
  used: number;
  remaining: number;
  resetAt: string;
  checkedAt: string;
};

export type DiscoveryWarmupStatus =
  | 'QUEUED'
  | 'RUNNING'
  | 'COMPLETED'
  | 'RESERVE_PROTECTED'
  | 'FAILED';

export type DiscoveryWarmup = {
  id: string;
  seedLogin: string;
  status: DiscoveryWarmupStatus;
  currentLogin: string | null;
  expandedUsers: number;
  discoveredUsers: number;
  pendingUsers: number;
  frontierTruncated: boolean;
  remainingRequests: number | null;
  reserveRequests: number;
  resetAt: string | null;
  startedAt: string;
  updatedAt: string;
  completedAt: string | null;
  lastError: string | null;
};

export type RepositoryContributor = {
  githubId: string;
  login: string;
  avatarUrl: string | null;
  url: string;
  contributions: number;
};

export type RepositoryContributorInsights = {
  fullName: string;
  contributors: RepositoryContributor[];
  sourceComplete: boolean;
  sourceDescription: string;
  cacheStatus: 'FRESH' | 'STALE' | 'REFRESHING' | 'REFRESH_FAILED';
  lastFetchedAt: string | null;
};

export type UserCommitRepository = {
  githubId: string;
  fullName: string;
  url: string;
  pushCount: number;
  commitCount: number;
  lastPushedAt: string;
};

export type UserCommitRepositoryInsights = {
  login: string;
  repositories: UserCommitRepository[];
  windowDays: number;
  sourceEventCount: number;
  sourceTruncated: boolean;
  sourceDescription: string;
  cacheStatus: 'FRESH' | 'STALE' | 'REFRESHING' | 'REFRESH_FAILED';
  lastFetchedAt: string | null;
};

export type ErrorEnvelope = {
  error: string;
  code?: string;
  remaining?: number;
  reserve?: number;
  requested_cost?: number;
  reset_at?: string;
  capacity_resource?: string;
  current_count?: number;
  incoming_count?: number;
  projected_count?: number;
  maximum_count?: number;
};

type GraphQLError = {
  message: string;
  extensions?: {
    code?: string;
    remaining?: number;
    reserve?: number;
    requestedCost?: number;
    resetAt?: string;
    capacityResource?: string;
    currentCount?: number;
    incomingCount?: number;
    projectedCount?: number;
    maximumCount?: number;
  };
};

type GraphQLResponse<T> = {
  data?: T | null;
  errors?: GraphQLError[];
};

export type FetchLike = typeof fetch;

export type ApiClientOptions = {
  baseUrl: string;
  fetch?: FetchLike;
};

type GitExploreApiErrorDetails = {
  code?: string;
  remaining?: number;
  reserve?: number;
  requestedCost?: number;
  resetAt?: string;
  capacityResource?: string;
  currentCount?: number;
  incomingCount?: number;
  projectedCount?: number;
  maximumCount?: number;
};

export class GitExploreApiError extends Error {
  status: number;
  code?: string;
  remaining?: number;
  reserve?: number;
  requestedCost?: number;
  resetAt?: string;
  capacityResource?: string;
  currentCount?: number;
  incomingCount?: number;
  projectedCount?: number;
  maximumCount?: number;

  constructor(
    message: string,
    status: number,
    details?: string | GitExploreApiErrorDetails
  ) {
    super(message);
    this.name = 'GitExploreApiError';
    this.status = status;
    const normalized = typeof details === 'string' ? { code: details } : details;
    this.code = normalized?.code;
    this.remaining = normalized?.remaining;
    this.reserve = normalized?.reserve;
    this.requestedCost = normalized?.requestedCost;
    this.resetAt = normalized?.resetAt;
    this.capacityResource = normalized?.capacityResource;
    this.currentCount = normalized?.currentCount;
    this.incomingCount = normalized?.incomingCount;
    this.projectedCount = normalized?.projectedCount;
    this.maximumCount = normalized?.maximumCount;
  }
}

export function createGitExploreApi(options: ApiClientOptions) {
  const baseUrl = options.baseUrl.replace(/\/$/, '');
  const fetchImpl = options.fetch ?? fetch;

  async function request<T>(
    path: string,
    init?: RequestInit,
    query?: Record<string, string | undefined>
  ): Promise<T> {
    const url = new URL(`${baseUrl}${path}`);
    for (const [key, value] of Object.entries(query ?? {})) {
      if (value) {
        url.searchParams.set(key, value);
      }
    }

    const response = await fetchImpl(url, {
      ...init,
      headers: {
        'content-type': 'application/json',
        ...(init?.headers ?? {})
      },
      credentials: 'include'
    });

    if (!response.ok) {
      const error = (await safeJson<ErrorEnvelope>(response)) ?? {
        error: response.statusText
      };
      throw new GitExploreApiError(error.error, response.status, {
        code: error.code,
        remaining: error.remaining,
        reserve: error.reserve,
        requestedCost: error.requested_cost,
        resetAt: error.reset_at,
        capacityResource: error.capacity_resource,
        currentCount: error.current_count,
        incomingCount: error.incoming_count,
        projectedCount: error.projected_count,
        maximumCount: error.maximum_count
      });
    }

    return (await response.json()) as T;
  }

  async function graphql<T>(
    query: string,
    variables: Record<string, unknown>
  ): Promise<T> {
    const response = await fetchImpl(`${baseUrl}/graphql`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json'
      },
      credentials: 'include',
      body: JSON.stringify({ query, variables })
    });
    const payload = await safeJson<GraphQLResponse<T> | ErrorEnvelope>(response);

    if (!response.ok) {
      const message =
        payload && 'error' in payload
          ? payload.error
          : `GraphQL request failed with status ${response.status}`;
      throw new GitExploreApiError(message, response.status);
    }

    if (!payload || 'error' in payload) {
      throw new GitExploreApiError(
        payload && 'error' in payload ? payload.error : 'GraphQL returned an empty response',
        500
      );
    }

    if (payload.errors?.length) {
      const extensions = payload.errors[0]?.extensions;
      throw new GitExploreApiError(
        payload.errors.map((error) => error.message).join('\n'),
        400,
        extensions
      );
    }

    if (!payload.data) {
      throw new GitExploreApiError('GraphQL response did not include data', 500);
    }

    return payload.data;
  }

  return {
    getHealth: () => request<{ status: string }>('/health', { method: 'GET' }, {}),
    getAuthStatus: () => request<ConnectionStatus>('/auth/status', { method: 'GET' }),
    startBrowserOAuth: (redirectTo: string) =>
      `${baseUrl}/auth/oauth/start?redirect_to=${encodeURIComponent(redirectTo)}`,
    logout: () => request<{ ok: boolean }>('/auth/logout', { method: 'POST' }),
    runSync: () => request<SyncSummary>('/sync/run', { method: 'POST' }),
    getSyncStatus: () => request<SyncStatus>('/sync/status', { method: 'GET' }),
    getBookmarks: () => request<Bookmark[]>('/bookmarks', { method: 'GET' }),
    addBookmark: (payload: {
      target: BookmarkTarget;
      categories: string[];
      note?: string | null;
    }) =>
      request<Bookmark>('/bookmarks', {
        method: 'POST',
        body: JSON.stringify(payload)
      }),
    getCategories: () => request<Category[]>('/categories', { method: 'GET' }),
    createCategory: (payload: { name: string; description?: string | null }) =>
      request<Category>('/categories', {
        method: 'POST',
        body: JSON.stringify(payload)
      }),
    explore: (seedType: 'user' | 'repository' | 'category', seedValue: string) =>
      request<ExplorationResult>('/explore', {
        method: 'POST',
        body: JSON.stringify({
          seed_type: seedType,
          seed_value: seedValue
        })
      }),
    getExplorationSnapshots: () =>
      request<ExplorationSnapshot[]>('/explore/snapshots', { method: 'GET' }),
    getRateLimit: async () => {
      const data = await graphql<{ rateLimit: GitHubRateLimit }>(
        `query RateLimit {
          rateLimit { limit used remaining resetAt checkedAt }
        }`,
        {}
      );
      return data.rateLimit;
    },
    getDiscoveryWarmup: async () => {
      const data = await graphql<{ discoveryWarmup: DiscoveryWarmup | null }>(
        `query DiscoveryWarmup {
          discoveryWarmup {
            id seedLogin status currentLogin
            expandedUsers discoveredUsers pendingUsers frontierTruncated
            remainingRequests reserveRequests resetAt
            startedAt updatedAt completedAt lastError
          }
        }`,
        {}
      );
      return data.discoveryWarmup;
    },
    startDiscoveryWarmup: async () => {
      const data = await graphql<{ startDiscoveryWarmup: DiscoveryWarmup }>(
        `mutation StartDiscoveryWarmup {
          startDiscoveryWarmup {
            id seedLogin status currentLogin
            expandedUsers discoveredUsers pendingUsers frontierTruncated
            remainingRequests reserveRequests resetAt
            startedAt updatedAt completedAt lastError
          }
        }`,
        {}
      );
      return data.startDiscoveryWarmup;
    },
    getRepositoryInsights: async (fullName: string, limit = 12) => {
      const data = await graphql<{
        repositoryInsights: RepositoryContributorInsights;
      }>(
        `query RepositoryInsights($fullName: String!, $limit: Int!) {
          repositoryInsights(fullName: $fullName, limit: $limit) {
            fullName
            contributors { githubId login avatarUrl url contributions }
            sourceComplete
            sourceDescription
            cacheStatus
            lastFetchedAt
          }
        }`,
        { fullName, limit }
      );
      return data.repositoryInsights;
    },
    getUserInsights: async (login: string, limit = 12) => {
      const data = await graphql<{ userInsights: UserCommitRepositoryInsights }>(
        `query UserInsights($login: String!, $limit: Int!) {
          userInsights(login: $login, limit: $limit) {
            login
            repositories {
              githubId fullName url pushCount commitCount lastPushedAt
            }
            windowDays
            sourceEventCount
            sourceTruncated
            sourceDescription
            cacheStatus
            lastFetchedAt
          }
        }`,
        { login, limit }
      );
      return data.userInsights;
    },
    getNeighborhood: async (login: string, limit = 24) => {
      const data = await graphql<{ neighborhood: UserNeighborhood }>(
        `query Neighborhood($login: String!, $limit: Int!) {
          neighborhood(login: $login, limit: $limit) {
            user { githubId login name url avatarUrl bio followersCount followingCount }
            followers { githubId login name url avatarUrl bio followersCount followingCount }
            following { githubId login name url avatarUrl bio followersCount followingCount }
            repositories {
              repository {
                githubId ownerLogin name fullName description htmlUrl
                stargazerCount forkCount primaryLanguage topics updatedAt archived fork
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
        }`,
        { login, limit }
      );
      return data.neighborhood;
    },
    expandUser: async (login: string, limit = 24) => {
      const data = await graphql<{ expandUser: UserNeighborhood }>(
        `mutation ExpandUser($login: String!, $limit: Int!) {
          expandUser(login: $login, limit: $limit) {
            user { githubId login name url avatarUrl bio followersCount followingCount }
            followers { githubId login name url avatarUrl bio followersCount followingCount }
            following { githubId login name url avatarUrl bio followersCount followingCount }
            repositories {
              repository {
                githubId ownerLogin name fullName description htmlUrl
                stargazerCount forkCount primaryLanguage topics updatedAt archived fork
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
        }`,
        { login, limit }
      );
      return data.expandUser;
    },
    saveRepository: async (
      fullName: string,
      categories: string[] = [],
      note?: string | null
    ) => {
      const data = await graphql<{ saveRepository: SavedRepository }>(
        `mutation SaveRepository($fullName: String!, $categories: [String!]!, $note: String) {
          saveRepository(fullName: $fullName, categories: $categories, note: $note) {
            id
            fullName
            categories
            note
            createdAt
          }
        }`,
        { fullName, categories, note: note ?? null }
      );
      return data.saveRepository;
    }
  };
}

async function safeJson<T>(response: Response): Promise<T | null> {
  try {
    return (await response.json()) as T;
  } catch {
    return null;
  }
}
