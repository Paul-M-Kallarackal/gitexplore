import type { UserCommitRepositoryInsights } from '@gitexplore/api-client';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { UserInsights } from './UserInsights';

const insight: UserCommitRepositoryInsights = {
  login: 'alice',
  repositories: [{
    githubId: 'repo-1',
    fullName: 'quiet-labs/signal-map',
    url: 'https://github.com/quiet-labs/signal-map',
    pushCount: 4,
    commitCount: 12,
    lastPushedAt: '2026-08-01T08:00:00Z',
  }],
  windowDays: 30,
  sourceEventCount: 4,
  sourceTruncated: false,
  sourceDescription: 'Recent public push events.',
  cacheStatus: 'FRESH',
  lastFetchedAt: '2026-08-01T08:01:00Z',
};

describe('UserInsights', () => {
  it('keeps recent-work exploration inside GitExplore with trail direction intact', () => {
    render(
      <MemoryRouter>
        <UserInsights insight={insight} trail={['alice']} direction="following" />
      </MemoryRouter>,
    );

    expect(screen.getByRole('link', { name: /quiet-labs\/signal-map/ })).toHaveAttribute(
      'href',
      '/app/repository/quiet-labs/signal-map?trail=alice&direction=following',
    );
    expect(screen.getByText('12 commits · 4 pushes')).toBeInTheDocument();
  });
});
