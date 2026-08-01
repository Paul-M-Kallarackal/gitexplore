import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ThemeProvider } from 'strawn';
import { describe, expect, it, vi } from 'vitest';

const getExplorationActivity = vi.hoisted(() => vi.fn());

vi.mock('../api', () => ({ api: { getExplorationActivity } }));
vi.mock('../auth', () => ({
  useAuth: () => ({
    status: {
      account: { login: 'alice', display_name: 'Alice' },
    },
  }),
}));

import { ExploreStartPage } from './ExploreStartPage';

describe('ExploreStartPage', () => {
  it('restores a visible recent person at the saved hop and direction', async () => {
    getExplorationActivity.mockResolvedValue({
      maxTrailDepth: 3,
      recentPeople: [
        {
          user: {
            githubId: '3',
            login: 'carol',
            name: 'Carol',
            url: 'https://github.com/carol',
            avatarUrl: null,
            bio: null,
            followersCount: 4,
            followingCount: 5,
          },
          trail: ['alice', 'bob', 'carol'],
          direction: 'following',
          lastViewedAt: '2026-08-01T10:00:00Z',
          visitCount: 1,
          visible: true,
        },
        {
          user: {
            githubId: '4',
            login: 'hidden-person',
            name: null,
            url: 'https://github.com/hidden-person',
            avatarUrl: null,
            bio: null,
            followersCount: null,
            followingCount: null,
          },
          trail: ['hidden-person'],
          direction: 'followers',
          lastViewedAt: '2026-08-01T09:00:00Z',
          visitCount: 1,
          visible: false,
        },
      ],
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    render(
      <ThemeProvider>
        <QueryClientProvider client={queryClient}>
          <MemoryRouter>
            <ExploreStartPage />
          </MemoryRouter>
        </QueryClientProvider>
      </ThemeProvider>,
    );

    const recentLink = await screen.findByRole('link', { name: /@carol/i });
    expect(recentLink).toHaveAttribute(
      'href',
      '/app/explore/carol?trail=alice%2Cbob%2Ccarol&direction=following',
    );
    expect(recentLink).toHaveTextContent('2 hops');
    expect(screen.queryByText('@hidden-person')).not.toBeInTheDocument();
  });
});
