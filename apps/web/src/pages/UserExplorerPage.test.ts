import type { UserNeighborhood } from '@gitexplore/api-client';
import { describe, expect, it } from 'vitest';

import { expansionTargetsLogin, neighborhoodNeedsExpansion } from './UserExplorerPage';

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
});
