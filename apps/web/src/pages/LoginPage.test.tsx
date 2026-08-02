import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ThemeProvider } from 'strawn';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const authState = vi.hoisted(() => ({
  status: undefined as undefined,
  loading: true,
  error: null as Error | null,
  refresh: vi.fn(async () => undefined),
}));

vi.mock('../auth', () => ({ useAuth: () => authState }));
vi.mock('../api', () => ({ api: { startBrowserOAuth: vi.fn(() => '/auth/oauth/start') } }));

import { LoginPage } from './LoginPage';

describe('LoginPage', () => {
  beforeEach(() => {
    authState.loading = true;
    authState.error = null;
    authState.refresh.mockClear();
  });

  it('keeps OAuth available while session status is pending', () => {
    render(<ThemeProvider><MemoryRouter><LoginPage /></MemoryRouter></ThemeProvider>);
    expect(screen.getByRole('link', { name: 'GitExplore home' }).querySelector('img')).toHaveAttribute(
      'src',
      '/images/gitexplore-wordmark.png',
    );
    const link = screen.getByText(/continue with github/i).closest('a');
    expect(link).not.toBeNull();
    expect(link).toHaveAttribute('href', '/auth/oauth/start');
    expect(link).not.toHaveAttribute('aria-disabled');
    expect(link).not.toHaveAttribute('tabindex');
    expect(link).toHaveClass('github-connect-link');
  });

  it('shows a backend error instead of treating it as signed out', () => {
    authState.loading = false;
    authState.error = new Error('Backend unavailable');
    render(<ThemeProvider><MemoryRouter><LoginPage /></MemoryRouter></ThemeProvider>);
    expect(screen.getByRole('alert')).toHaveTextContent('Backend unavailable');
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });
});
