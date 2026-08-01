import { GitExploreApiError, type ExplorationActivity, type GraphUser, type UserNeighborhood } from '@gitexplore/api-client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { ThemeProvider } from 'strawn';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  getNeighborhood: vi.fn(),
  getExplorationActivity: vi.fn(),
  recordPersonVisit: vi.fn(),
  setRecentPersonVisible: vi.fn(),
  getUserInsights: vi.fn(),
  expandUser: vi.fn(),
  saveRepository: vi.fn(),
}));

vi.mock('../api', () => ({ api: apiMocks }));

import { UserExplorerPage, expansionTargetsLogin, neighborhoodNeedsExpansion } from './UserExplorerPage';

function cachedNeighborhood(lastFetchedAt: string | null): UserNeighborhood {
  return {
    user: {
      githubId: '1',
      login: 'quiet-user',
      name: null,
      url: 'https://github.com/quiet-user',
      avatarUrl: null,
      bio: null,
      followersCount: 0,
      followingCount: 0,
    },
    followers: [],
    following: [],
    repositories: [],
    cacheStatus: 'FRESH',
    lastFetchedAt,
    coverage: {
      followersComplete: true,
      followingComplete: true,
      starredRepositoriesComplete: true,
      repositoriesComplete: true,
    },
  };
}

function activityFor(user: GraphUser, visible = true, maxTrailDepth = 1): ExplorationActivity {
  return {
    maxTrailDepth,
    recentPeople: [{
      user,
      trail: maxTrailDepth ? ['alice', user.login] : [user.login],
      direction: 'following',
      lastViewedAt: '2026-08-01T10:00:00Z',
      visitCount: 1,
      visible,
    }],
  };
}

function insightFor(login: string) {
  return {
    login,
    repositories: [],
    windowDays: 30,
    sourceEventCount: 0,
    sourceTruncated: false,
    sourceDescription: 'Recent public push events.',
    cacheStatus: 'FRESH' as const,
    lastFetchedAt: '2026-08-01T10:00:00Z',
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderExplorer(
  path = '/app/explore/bob?trail=alice,bob&direction=following',
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  }),
) {

  render(
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={[path]}>
          <Routes>
            <Route path="/app/explore/:login" element={<UserExplorerPage />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    </ThemeProvider>,
  );

  return queryClient;
}

beforeEach(() => {
  Object.values(apiMocks).forEach((mock) => mock.mockReset());
  apiMocks.getUserInsights.mockImplementation(() => new Promise(() => undefined));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('neighborhoodNeedsExpansion', () => {
  it('expands a missing or never-fetched node', () => {
    expect(neighborhoodNeedsExpansion(null)).toBe(true);
    expect(neighborhoodNeedsExpansion(cachedNeighborhood(null))).toBe(true);
  });

  it('does not spend another request on a fresh, legitimately empty node', () => {
    expect(neighborhoodNeedsExpansion(cachedNeighborhood('2026-08-01T05:00:00Z'))).toBe(false);
  });

  it('scopes expansion status to the currently requested login', () => {
    expect(expansionTargetsLogin('alice', 'alice')).toBe(true);
    expect(expansionTargetsLogin('Alice', 'alice')).toBe(true);
    expect(expansionTargetsLogin('alice', 'bob')).toBe(false);
    expect(expansionTargetsLogin(undefined, 'bob')).toBe(false);
  });

  it('records the complete hop and removes a person directly from their page', async () => {
    const neighborhood = cachedNeighborhood('2026-08-01T05:00:00Z');
    neighborhood.user = {
      ...neighborhood.user,
      githubId: '2',
      login: 'bob',
      name: 'Bob',
      url: 'https://github.com/bob',
    };
    let serverActivity: ExplorationActivity = {
      maxTrailDepth: 1,
      recentPeople: [{
        user: neighborhood.user,
        trail: ['alice', 'bob'],
        direction: 'following' as const,
        lastViewedAt: '2026-08-01T10:00:00Z',
        visitCount: 1,
        visible: true,
      }],
    };
    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.getExplorationActivity.mockImplementation(() => Promise.resolve(serverActivity));
    apiMocks.recordPersonVisit.mockImplementation(() => Promise.resolve(serverActivity));
    apiMocks.setRecentPersonVisible.mockImplementation((_login: string, visible: boolean) => {
      serverActivity = {
        ...serverActivity,
        recentPeople: serverActivity.recentPeople.map((person) => ({ ...person, visible })),
      };
      return Promise.resolve(serverActivity);
    });
    renderExplorer();

    await waitFor(() => {
      expect(apiMocks.recordPersonVisit).toHaveBeenCalledWith(
        'bob',
        ['alice', 'bob'],
        'following',
      );
    });
    const removeButton = await screen.findByRole('button', { name: 'Remove from recent' });
    fireEvent.click(removeButton);
    await waitFor(() => {
      expect(apiMocks.setRecentPersonVisible).toHaveBeenCalledWith('bob', false);
    });
    expect(await screen.findByRole('button', { name: 'Add to recent' })).toBeInTheDocument();
    expect(apiMocks.getExplorationActivity).toHaveBeenCalledTimes(1);
  });

  it('does not present an absent activity record as already visible', async () => {
    const neighborhood = cachedNeighborhood('2026-08-01T05:00:00Z');
    neighborhood.user = { ...neighborhood.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    const visit = deferred<ExplorationActivity>();
    const emptyActivity: ExplorationActivity = { maxTrailDepth: 0, recentPeople: [] };
    const visibleActivity = activityFor(neighborhood.user);
    let serverActivity = emptyActivity;

    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.getExplorationActivity.mockImplementation(() => Promise.resolve(serverActivity));
    apiMocks.recordPersonVisit.mockImplementation(() => visit.promise);
    renderExplorer();

    const pendingButton = await screen.findByRole('button', { name: 'Saving recent status' });
    expect(pendingButton).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Remove from recent' })).not.toBeInTheDocument();

    serverActivity = visibleActivity;
    visit.resolve(visibleActivity);
    expect(await screen.findByRole('button', { name: 'Remove from recent' })).toBeInTheDocument();
    expect(apiMocks.getExplorationActivity).toHaveBeenCalledTimes(1);
  });

  it('does not automatically retry a transient visit failure', async () => {
    const neighborhood = cachedNeighborhood('2026-08-01T05:00:00Z');
    neighborhood.user = { ...neighborhood.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    const emptyActivity: ExplorationActivity = { maxTrailDepth: 0, recentPeople: [] };

    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.getExplorationActivity.mockResolvedValue(emptyActivity);
    apiMocks.recordPersonVisit.mockRejectedValueOnce(new GitExploreApiError('Temporary history outage', 503));
    renderExplorer();

    expect(await screen.findByText('Recent person was not saved')).toBeInTheDocument();
    expect(apiMocks.recordPersonVisit).toHaveBeenCalledTimes(1);
    await new Promise((resolve) => setTimeout(resolve, 200));
    expect(apiMocks.recordPersonVisit).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });

  it('offers an explicit retry after a permanent visit failure', async () => {
    const neighborhood = cachedNeighborhood('2026-08-01T05:00:00Z');
    neighborhood.user = { ...neighborhood.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    const emptyActivity: ExplorationActivity = { maxTrailDepth: 0, recentPeople: [] };
    const visibleActivity = activityFor(neighborhood.user);
    let serverActivity = emptyActivity;

    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.getExplorationActivity.mockImplementation(() => Promise.resolve(serverActivity));
    apiMocks.recordPersonVisit
      .mockRejectedValueOnce(new GitExploreApiError('History write was rejected', 400))
      .mockImplementationOnce(() => {
        serverActivity = visibleActivity;
        return Promise.resolve(visibleActivity);
      });
    renderExplorer();

    expect(await screen.findByText('Recent person was not saved')).toBeInTheDocument();
    expect(apiMocks.recordPersonVisit).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));

    await waitFor(() => expect(apiMocks.recordPersonVisit).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole('button', { name: 'Remove from recent' })).toBeInTheDocument();
  });

  it('shows a rank skeleton until private expedition progress loads', async () => {
    const neighborhood = cachedNeighborhood('2026-08-01T05:00:00Z');
    neighborhood.user = { ...neighborhood.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.getExplorationActivity.mockImplementation(() => new Promise(() => undefined));
    apiMocks.recordPersonVisit.mockImplementation(() => new Promise(() => undefined));
    renderExplorer();

    expect(await screen.findByLabelText('Loading expedition progress')).toHaveAttribute('aria-busy', 'true');
  });

  it('does not request recent work until the explicit fallback is activated', async () => {
    const neighborhood = cachedNeighborhood('2026-08-01T05:00:00Z');
    neighborhood.user = { ...neighborhood.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    const visibleActivity = activityFor(neighborhood.user);
    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.getExplorationActivity.mockResolvedValue(visibleActivity);
    apiMocks.recordPersonVisit.mockResolvedValue(visibleActivity);
    apiMocks.getUserInsights.mockResolvedValue(insightFor('bob'));
    renderExplorer();

    const loadButton = await screen.findByRole('button', { name: 'Load recent work' });
    fireEvent.mouseEnter(loadButton);
    expect(apiMocks.getUserInsights).not.toHaveBeenCalled();
    fireEvent.click(loadButton);

    await waitFor(() => expect(apiMocks.getUserInsights).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole('heading', { name: 'Where @bob is pushing' })).toBeInTheDocument();
  });

  it('requests recent work once when its section approaches the viewport', async () => {
    let intersectionCallback: IntersectionObserverCallback | undefined;
    let observerOptions: IntersectionObserverInit | undefined;
    const observe = vi.fn();
    const disconnect = vi.fn();
    class MockIntersectionObserver {
      readonly root = null;
      readonly rootMargin = '240px 0px';
      readonly thresholds = [0];

      constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        intersectionCallback = callback;
        observerOptions = options;
      }

      observe = observe;
      disconnect = disconnect;
      unobserve = vi.fn();
      takeRecords = () => [];
    }
    vi.stubGlobal('IntersectionObserver', MockIntersectionObserver);

    const neighborhood = cachedNeighborhood('2026-08-01T05:00:00Z');
    neighborhood.user = { ...neighborhood.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    const visibleActivity = activityFor(neighborhood.user);
    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.getExplorationActivity.mockResolvedValue(visibleActivity);
    apiMocks.recordPersonVisit.mockResolvedValue(visibleActivity);
    apiMocks.getUserInsights.mockResolvedValue(insightFor('bob'));
    renderExplorer();

    await waitFor(() => expect(observe).toHaveBeenCalledTimes(1));
    expect(observerOptions?.rootMargin).toBe('240px 0px');
    expect(apiMocks.getUserInsights).not.toHaveBeenCalled();
    intersectionCallback?.([{ isIntersecting: false } as IntersectionObserverEntry], {} as IntersectionObserver);
    expect(apiMocks.getUserInsights).not.toHaveBeenCalled();
    intersectionCallback?.([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver);

    await waitFor(() => expect(apiMocks.getUserInsights).toHaveBeenCalledTimes(1));
    intersectionCallback?.([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(apiMocks.getUserInsights).toHaveBeenCalledTimes(1);
    expect(disconnect).toHaveBeenCalled();
  });

  it('settles expansion without waiting for the background rate-limit refresh', async () => {
    const neighborhood = cachedNeighborhood(null);
    neighborhood.user = { ...neighborhood.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    const refreshedNeighborhood = { ...neighborhood, lastFetchedAt: '2026-08-01T10:00:00Z' };
    const visibleActivity = activityFor(neighborhood.user);
    const expansion = deferred<UserNeighborhood>();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const originalInvalidate = queryClient.invalidateQueries.bind(queryClient);
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries').mockImplementation((filters, options) => {
      if (JSON.stringify(filters?.queryKey) === JSON.stringify(['github-rate-limit'])) {
        return new Promise(() => undefined);
      }
      return originalInvalidate(filters, options);
    });
    apiMocks.getNeighborhood.mockResolvedValue(neighborhood);
    apiMocks.expandUser.mockImplementation(() => expansion.promise);
    apiMocks.getExplorationActivity.mockResolvedValue(visibleActivity);
    apiMocks.recordPersonVisit.mockResolvedValue(visibleActivity);
    renderExplorer('/app/explore/bob', queryClient);

    const refreshButton = await screen.findByRole('button', { name: 'Refresh node' });
    await waitFor(() => expect(refreshButton).toHaveAttribute('aria-busy', 'true'));
    expansion.resolve(refreshedNeighborhood);

    await waitFor(() => expect(refreshButton).not.toHaveAttribute('aria-busy'));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['github-rate-limit'] });
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ['user-insights', 'bob'] });
    expect(apiMocks.getUserInsights).not.toHaveBeenCalled();
  });

  it('serializes visits so an eagerly resolved newer response is installed last', async () => {
    const alice = cachedNeighborhood('2026-08-01T05:00:00Z');
    alice.user = { ...alice.user, githubId: '1', login: 'alice', name: 'Alice', url: 'https://github.com/alice' };
    const bob = cachedNeighborhood('2026-08-01T05:00:00Z');
    bob.user = { ...bob.user, githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob' };
    alice.followers = [bob.user];
    const aliceVisit = deferred<ExplorationActivity>();
    const bobVisit = deferred<ExplorationActivity>();
    const emptyActivity: ExplorationActivity = { maxTrailDepth: 0, recentPeople: [] };

    apiMocks.getNeighborhood.mockImplementation((login: string) => Promise.resolve(login === 'alice' ? alice : bob));
    apiMocks.getExplorationActivity.mockResolvedValue(emptyActivity);
    apiMocks.recordPersonVisit.mockImplementation((login: string) => login === 'alice' ? aliceVisit.promise : bobVisit.promise);
    const queryClient = renderExplorer('/app/explore/alice');

    await waitFor(() => expect(apiMocks.recordPersonVisit).toHaveBeenCalledWith('alice', ['alice'], 'followers'));
    fireEvent.click(await screen.findByRole('link', { name: /Bob/i }));

    const bobActivity = activityFor(bob.user);
    const aliceActivity = activityFor(alice.user, true, 0);
    const finalActivity: ExplorationActivity = {
      maxTrailDepth: 1,
      recentPeople: [bobActivity.recentPeople[0]!, aliceActivity.recentPeople[0]!],
    };
    bobVisit.resolve(finalActivity);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(apiMocks.recordPersonVisit).toHaveBeenCalledTimes(1);

    aliceVisit.resolve(aliceActivity);

    await waitFor(() => expect(apiMocks.recordPersonVisit).toHaveBeenCalledWith('bob', ['alice', 'bob'], 'followers'));
    expect(await screen.findByRole('button', { name: 'Remove from recent' })).toBeInTheDocument();

    await waitFor(() => {
      const cached = queryClient.getQueryData<ExplorationActivity>(['exploration-activity']);
      expect(cached?.recentPeople.map((person) => person.user.login)).toEqual(['bob', 'alice']);
    });
    expect(screen.getByRole('button', { name: 'Remove from recent' })).toBeInTheDocument();
    expect(apiMocks.getExplorationActivity).toHaveBeenCalledTimes(1);
  });
});
