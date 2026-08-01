import type { DiscoveryWarmup } from '@gitexplore/api-client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from 'strawn';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  getDiscoveryWarmup: vi.fn(),
  startDiscoveryWarmup: vi.fn(),
  getRateLimit: vi.fn(),
  logout: vi.fn(),
}));

vi.mock('../api', () => ({ api: apiMocks }));
vi.mock('../auth', () => ({
  useAuth: () => ({
    status: { account: { display_name: 'Paul', login: 'paul' } },
  }),
  useLogout: () => apiMocks.logout,
}));

import { SettingsPage } from './SettingsPage';

const completedWarmup: DiscoveryWarmup = {
  id: 'warmup-001',
  seedLogin: 'paul',
  status: 'COMPLETED',
  currentLogin: null,
  expandedUsers: 24,
  discoveredUsers: 31,
  pendingUsers: 0,
  frontierTruncated: false,
  remainingRequests: 1_240,
  reserveRequests: 1_000,
  resetAt: '2026-08-01T06:00:00Z',
  startedAt: '2026-08-01T05:00:00Z',
  updatedAt: '2026-08-01T05:10:00Z',
  completedAt: '2026-08-01T05:10:00Z',
  lastError: null,
};

function renderSettings() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const invalidateQueries = vi.spyOn(queryClient, 'invalidateQueries');

  render(
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <SettingsPage />
      </QueryClientProvider>
    </ThemeProvider>,
  );

  return { invalidateQueries };
}

describe('SettingsPage discovery map', () => {
  beforeEach(() => {
    apiMocks.getDiscoveryWarmup.mockReset().mockResolvedValue(null);
    apiMocks.startDiscoveryWarmup.mockReset().mockResolvedValue(completedWarmup);
    apiMocks.getRateLimit.mockReset().mockResolvedValue({
      limit: 5_000,
      used: 480,
      remaining: 4_520,
      resetAt: '2026-08-01T06:00:00Z',
      checkedAt: '2026-08-01T05:30:00Z',
    });
    apiMocks.logout.mockReset().mockResolvedValue(undefined);
  });

  it('starts the durable map and explains its hourly protected reserve', async () => {
    const user = userEvent.setup();
    const { invalidateQueries } = renderSettings();

    expect(await screen.findByText('Ready to map')).toBeInTheDocument();
    expect(screen.getByText(/protected 1,000-request reserve/i)).toHaveTextContent(/continue after GitHub's hourly reset/i);

    await user.click(screen.getByRole('button', { name: 'Start mapping' }));

    expect(apiMocks.startDiscoveryWarmup).toHaveBeenCalledOnce();
    expect(await screen.findByText('Map is warm')).toBeInTheDocument();
    expect(within(screen.getByText('Expanded').parentElement!).getByText('24')).toBeInTheDocument();
    expect(within(screen.getByText('REST left').parentElement!).getByText('1,240')).toBeInTheDocument();
    await waitFor(() => {
      expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ['github-rate-limit'] });
      expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ['user-neighborhood'] });
    });
  });
});
