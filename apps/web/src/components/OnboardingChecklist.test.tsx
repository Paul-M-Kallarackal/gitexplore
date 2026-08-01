import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { ThemeProvider } from 'strawn';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  getOnboardingProgress: vi.fn(),
  beginOnboarding: vi.fn(),
  dismissOnboarding: vi.fn(),
  restartOnboarding: vi.fn(),
  completeOnboarding: vi.fn(),
  getDiscoveryWarmup: vi.fn(),
  getRateLimit: vi.fn(),
  startDiscoveryWarmup: vi.fn(),
}));

vi.mock('../api', () => ({ api }));
vi.mock('../auth', () => ({
  useAuth: () => ({
    status: { account: { login: 'alice', display_name: 'Alice' } },
  }),
}));

import { OnboardingProvider } from '../onboarding';
import { OnboardingChecklist, OnboardingCompletion } from './OnboardingChecklist';

const notStarted = {
  version: 1,
  status: 'NOT_STARTED' as const,
  startedAt: null,
  completedAt: null,
  dismissedAt: null,
  openedTrailhead: false,
  followedConnection: false,
  savedRepository: false,
  mappingStarted: false,
};

const inProgress = {
  ...notStarted,
  status: 'IN_PROGRESS' as const,
  startedAt: '2026-08-01T10:00:00Z',
};

function renderOnboarding() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={['/app/explore']}>
          <OnboardingProvider>
            <OnboardingCompletion />
            <OnboardingChecklist />
          </OnboardingProvider>
        </MemoryRouter>
      </QueryClientProvider>
    </ThemeProvider>,
  );
}

describe('OnboardingChecklist', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getOnboardingProgress.mockResolvedValue(notStarted);
    api.beginOnboarding.mockResolvedValue(inProgress);
    api.dismissOnboarding.mockResolvedValue({ ...inProgress, status: 'DISMISSED' });
    api.completeOnboarding.mockResolvedValue({ ...inProgress, status: 'COMPLETED' });
    api.getDiscoveryWarmup.mockResolvedValue(null);
    api.getRateLimit.mockResolvedValue({
      limit: 5000,
      used: 800,
      remaining: 4200,
      resetAt: '2026-08-01T11:00:00Z',
      checkedAt: '2026-08-01T10:00:00Z',
    });
    api.startDiscoveryWarmup.mockResolvedValue({
      id: 'warmup-1', seedLogin: 'alice', status: 'QUEUED', currentLogin: null,
      expandedUsers: 0, discoveredUsers: 1, pendingUsers: 1, frontierTruncated: false,
      remainingRequests: 4200, reserveRequests: 1000, resetAt: null,
      startedAt: '2026-08-01T10:00:00Z', updatedAt: '2026-08-01T10:00:00Z',
      completedAt: null, lastError: null,
    });
  });

  it('begins once, recommends the connected trailhead, and keeps mapping opt-in', async () => {
    const user = userEvent.setup();
    renderOnboarding();

    expect(await screen.findByRole('heading', { name: 'Your first GitExplore trail' })).toBeInTheDocument();
    await waitFor(() => expect(api.beginOnboarding).toHaveBeenCalledTimes(1));
    expect(screen.getByRole('link', { name: /start from @alice/i })).toHaveAttribute('href', '/app/explore/alice?trail=alice');
    expect(api.startDiscoveryWarmup).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Start mapping' }));
    expect(api.startDiscoveryWarmup).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('Mapping network')).toBeInTheDocument();
  });

  it('persists an explicit skip', async () => {
    const user = userEvent.setup();
    api.getOnboardingProgress.mockResolvedValue(inProgress);
    renderOnboarding();

    await user.click(await screen.findByRole('button', { name: 'Skip onboarding' }));
    await waitFor(() => expect(api.dismissOnboarding).toHaveBeenCalledTimes(1));
  });

  it('completes only after all real-action flags are present', async () => {
    api.getOnboardingProgress.mockResolvedValue({
      ...inProgress,
      openedTrailhead: true,
      followedConnection: true,
      savedRepository: true,
    });
    renderOnboarding();

    await waitFor(() => expect(api.completeOnboarding).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Your first find is saved')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open Saved' })).toHaveAttribute('href', '/app/saved');
  });
});
