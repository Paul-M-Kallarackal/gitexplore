import { useQuery } from '@tanstack/react-query';
import { Link, NavLink, Outlet } from 'react-router-dom';
import { Avatar, Badge, Progress } from 'strawn';
import { BookmarkIcon, GitHubIcon, SearchIcon, SettingsIcon } from 'strawn-icons';

import { api } from '../api';
import { useAuth } from '../auth';
import { OnboardingProvider } from '../onboarding';
import { OnboardingChecklist, OnboardingCompletion } from './OnboardingChecklist';

const navItems = [
  { to: '/app/explore', label: 'Explore', icon: SearchIcon },
  { to: '/app/saved', label: 'Saved', icon: BookmarkIcon },
  { to: '/app/settings', label: 'Settings', icon: SettingsIcon },
] as const;

export function AppShell() {
  return <OnboardingProvider><AppShellContent /></OnboardingProvider>;
}

function AppShellContent() {
  const { status } = useAuth();
  const rateQuery = useQuery({
    queryKey: ['github-rate-limit'],
    queryFn: () => api.getRateLimit(),
    staleTime: 60_000,
    refetchInterval: 5 * 60_000,
    retry: 1,
  });

  return (
    <div className="app-frame">
      <a className="skip-link" href="#main-content">Skip to content</a>
      <aside className="app-rail">
        <Link className="wordmark" to="/app/explore" aria-label="GitExplore Explore">
          <span className="wordmark-mark" aria-hidden="true"><GitHubIcon size={18} /></span>
          <span>GitExplore</span>
        </Link>
        <nav className="primary-nav" aria-label="Primary">
          {navItems.map(({ to, label, icon: Icon }) => (
            <NavLink key={to} to={to} className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`}>
              <Icon aria-hidden="true" size={18} /><span>{label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="rail-account">
          {rateQuery.data ? (
            <div className="rail-budget">
              <Progress label="GitHub requests left" value={rateQuery.data.remaining} max={rateQuery.data.limit} size="sm" />
              <span className="numeric-caption">{rateQuery.data.remaining.toLocaleString()} / {rateQuery.data.limit.toLocaleString()}</span>
            </div>
          ) : (
            <Badge tone={rateQuery.isError ? 'error' : 'neutral'}>{rateQuery.isError ? 'Rate unavailable' : 'Reading rate limit'}</Badge>
          )}
          <div className="account-row">
            <Avatar
              name={status?.account?.display_name || status?.account?.login || 'GitHub account'}
              src={status?.account?.login ? `https://github.com/${encodeURIComponent(status.account.login)}.png?size=80` : undefined}
              size="sm"
            />
            <div className="account-copy">
              <strong>{status?.account?.display_name || status?.account?.login}</strong>
              <span>@{status?.account?.login}</span>
            </div>
          </div>
        </div>
      </aside>
      <header className="mobile-header">
        <Link className="wordmark" to="/app/explore" aria-label="GitExplore Explore">
          <span className="wordmark-mark" aria-hidden="true"><GitHubIcon size={17} /></span><span>GitExplore</span>
        </Link>
        {rateQuery.data ? <span className="numeric-caption">{rateQuery.data.remaining.toLocaleString()} requests</span> : null}
      </header>
      <main id="main-content" className="app-main" tabIndex={-1}>
        <OnboardingCompletion />
        <OnboardingChecklist />
        <Outlet />
      </main>
      <nav className="mobile-nav" aria-label="Primary">
        {navItems.map(({ to, label, icon: Icon }) => (
          <NavLink key={to} to={to} className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`}>
            <Icon aria-hidden="true" size={18} /><span>{label}</span>
          </NavLink>
        ))}
      </nav>
    </div>
  );
}
