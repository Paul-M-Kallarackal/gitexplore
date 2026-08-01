import { describe, expect, it, vi } from 'vitest';

import { createGitExploreApi, GitExploreApiError } from './index';

describe('createGitExploreApi', () => {
  it('uses cookie-based auth and parses successful JSON responses', async () => {
    const fetchMock = vi.fn(async (input: URL | RequestInfo) => {
      expect(String(input)).not.toContain('user_id=');
      return new Response(
        JSON.stringify({ authenticated: true, app_user_id: 'app-user-1', connected: false, account: null }),
        {
        status: 200,
        headers: { 'content-type': 'application/json' }
        }
      );
    });

    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const result = await api.getAuthStatus();
    expect(result.connected).toBe(false);
    expect(result.app_user_id).toBe('app-user-1');
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ credentials: 'include' });
    expect(api.startBrowserOAuth('http://localhost:3000/app')).toBe(
      'http://localhost:4000/auth/oauth/start?redirect_to=http%3A%2F%2Flocalhost%3A3000%2Fapp'
    );
  });

  it('throws a typed API error when the server returns an error envelope', async () => {
    const fetchMock = vi.fn(async (input: URL | RequestInfo, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost:4000/explore');
      expect(init).toMatchObject({ method: 'POST', credentials: 'include' });
      expect(JSON.parse(String(init?.body))).toEqual({
        seed_type: 'user',
        seed_value: ''
      });
      return new Response(JSON.stringify({ error: 'validation error: missing seed' }), {
        status: 400,
        headers: { 'content-type': 'application/json' }
      });
    });

    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    await expect(api.explore('user', '')).rejects.toBeInstanceOf(GitExploreApiError);
  });

  it('preserves reset-aware REST reserve details', async () => {
    const fetchMock = vi.fn(async () =>
      Response.json(
        {
          error: 'GitHub REST reserve is active',
          code: 'RATE_BUDGET_RESERVED',
          remaining: 1012,
          reserve: 1000,
          requested_cost: 13,
          reset_at: '2026-08-01T12:00:00Z'
        },
        { status: 429 }
      )
    );
    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const error = await api.runSync().catch((reason) => reason as GitExploreApiError);

    expect(error).toBeInstanceOf(GitExploreApiError);
    expect(error).toMatchObject({
      status: 429,
      code: 'RATE_BUDGET_RESERVED',
      remaining: 1012,
      reserve: 1000,
      requestedCost: 13,
      resetAt: '2026-08-01T12:00:00Z'
    });
  });

  it('sends graph navigation through GraphQL with cookie credentials', async () => {
    const fetchMock = vi.fn(async (_input: URL | RequestInfo, init?: RequestInit) => {
      expect(init).toMatchObject({
        method: 'POST',
        credentials: 'include'
      });
      expect(JSON.parse(String(init?.body))).toMatchObject({
        variables: { login: 'octocat', limit: 12 }
      });
      return new Response(
        JSON.stringify({
          data: {
            neighborhood: {
              user: {
                githubId: '1',
                login: 'octocat',
                name: 'The Octocat',
                url: 'https://github.com/octocat',
                avatarUrl: null
              },
              followers: [],
              following: [],
              repositories: [],
              cacheStatus: 'FRESH',
              lastFetchedAt: null,
              coverage: {
                followersComplete: true,
                followingComplete: true,
                starredRepositoriesComplete: true,
                repositoriesComplete: true
              }
            }
          }
        }),
        { status: 200, headers: { 'content-type': 'application/json' } }
      );
    });
    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const result = await api.getNeighborhood('octocat', 12);

    expect(result.user.login).toBe('octocat');
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe('http://localhost:4000/graphql');
  });

  it('preserves GraphQL error codes for safe cache-miss handling', async () => {
    const fetchMock = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          errors: [
            {
              message: 'not found: github user `octocat` is not present in the shared graph',
              extensions: { code: 'NOT_FOUND' }
            }
          ]
        }),
        { status: 200, headers: { 'content-type': 'application/json' } }
      );
    });
    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const error = await api.getNeighborhood('octocat').catch((reason) => reason);

    expect(error).toBeInstanceOf(GitExploreApiError);
    expect((error as GitExploreApiError).code).toBe('NOT_FOUND');
  });

  it('preserves reset-aware GraphQL reserve details', async () => {
    const fetchMock = vi.fn(async () =>
      Response.json({
        errors: [
          {
            message: 'GitHub REST reserve is active',
            extensions: {
              code: 'RATE_BUDGET_RESERVED',
              remaining: 1000,
              reserve: 1000,
              requestedCost: 1,
              resetAt: '2026-08-01T12:00:00Z'
            }
          }
        ]
      })
    );
    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const error = await api
      .getRepositoryInsights('acme/tool')
      .catch((reason) => reason as GitExploreApiError);

    expect(error).toMatchObject({
      code: 'RATE_BUDGET_RESERVED',
      remaining: 1000,
      reserve: 1000,
      requestedCost: 1,
      resetAt: '2026-08-01T12:00:00Z'
    });
  });

  it('starts and reads the authenticated discovery warmup through GraphQL', async () => {
    const fetchMock = vi.fn(async (_input: URL | RequestInfo, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body)) as { query: string };
      const warmup = {
        id: 'warmup-1',
        seedLogin: 'octocat',
        status: 'QUEUED',
        currentLogin: null,
        expandedUsers: 0,
        discoveredUsers: 1,
        pendingUsers: 1,
        frontierTruncated: false,
        remainingRequests: null,
        reserveRequests: 1000,
        resetAt: null,
        startedAt: '2026-08-01T10:00:00Z',
        updatedAt: '2026-08-01T10:00:00Z',
        completedAt: null,
        lastError: null
      };
      return Response.json({
        data: body.query.includes('mutation StartDiscoveryWarmup')
          ? { startDiscoveryWarmup: warmup }
          : { discoveryWarmup: warmup }
      });
    });
    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const started = await api.startDiscoveryWarmup();
    const status = await api.getDiscoveryWarmup();

    expect(started.id).toBe('warmup-1');
    expect(status).toMatchObject({ seedLogin: 'octocat', reserveRequests: 1000 });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ credentials: 'include' });
  });

  it('preserves GraphQL database-capacity details', async () => {
    const fetchMock = vi.fn(async () =>
      Response.json({
        errors: [
          {
            message: 'Neo4j capacity gate rejected graph import',
            extensions: {
              code: 'GRAPH_CAPACITY_EXCEEDED',
              capacityResource: 'relationships',
              currentCount: 379500,
              incomingCount: 600,
              projectedCount: 380100,
              maximumCount: 380000
            }
          }
        ]
      })
    );
    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const error = await api
      .expandUser('octocat')
      .catch((reason) => reason as GitExploreApiError);

    expect(error).toMatchObject({
      code: 'GRAPH_CAPACITY_EXCEEDED',
      capacityResource: 'relationships',
      currentCount: 379500,
      incomingCount: 600,
      projectedCount: 380100,
      maximumCount: 380000
    });
  });

  it('exposes authenticated rate and cache-aware insight queries', async () => {
    const fetchMock = vi.fn(async (_input: URL | RequestInfo, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body)) as {
        query: string;
        variables: Record<string, unknown>;
      };
      if (body.query.includes('query RateLimit')) {
        return Response.json({
          data: {
            rateLimit: {
              limit: 5000,
              used: 125,
              remaining: 4875,
              resetAt: '2026-08-01T12:00:00Z',
              checkedAt: '2026-08-01T11:30:00Z'
            }
          }
        });
      }
      if (body.query.includes('query RepositoryInsights')) {
        expect(body.variables).toEqual({ fullName: 'acme/tool', limit: 8 });
        return Response.json({
          data: {
            repositoryInsights: {
              fullName: 'acme/tool',
              contributors: [
                {
                  githubId: '1',
                  login: 'octocat',
                  avatarUrl: null,
                  url: 'https://github.com/octocat',
                  contributions: 42
                }
              ],
              sourceComplete: true,
              sourceDescription: 'repository history',
              cacheStatus: 'FRESH',
              lastFetchedAt: '2026-08-01T11:00:00Z'
            }
          }
        });
      }
      expect(body.variables).toEqual({ login: 'octocat', limit: 6 });
      return Response.json({
        data: {
          userInsights: {
            login: 'octocat',
            repositories: [
              {
                githubId: '2',
                fullName: 'acme/tool',
                url: 'https://github.com/acme/tool',
                pushCount: 3,
                commitCount: 7,
                lastPushedAt: '2026-08-01T10:00:00Z'
              }
            ],
            windowDays: 30,
            sourceEventCount: 12,
            sourceTruncated: false,
            sourceDescription: 'public PushEvent activity',
            cacheStatus: 'FRESH',
            lastFetchedAt: '2026-08-01T11:00:00Z'
          }
        }
      });
    });
    const api = createGitExploreApi({
      baseUrl: 'http://localhost:4000',
      fetch: fetchMock as typeof fetch
    });

    const rate = await api.getRateLimit();
    const repository = await api.getRepositoryInsights('acme/tool', 8);
    const user = await api.getUserInsights('octocat', 6);

    expect(rate.remaining).toBe(4875);
    expect(repository.contributors[0]?.contributions).toBe(42);
    expect(user.windowDays).toBe(30);
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });
});
