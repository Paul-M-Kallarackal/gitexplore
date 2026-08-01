import { lazy, Suspense, type ReactNode } from 'react';
import { Link, Navigate, Outlet, Route, Routes, useLocation } from 'react-router-dom';
import { Alert, Button, Heading, Skeleton, Text } from 'strawn';
import { CircleAlertIcon, RefreshIcon } from 'strawn-icons';

import { useAuth } from './auth';

const AppShell = lazy(async () => ({ default: (await import('./components/AppShell')).AppShell }));
const LoginPage = lazy(async () => ({ default: (await import('./pages/LoginPage')).LoginPage }));
const ExploreStartPage = lazy(async () => ({ default: (await import('./pages/ExploreStartPage')).ExploreStartPage }));
const UserExplorerPage = lazy(async () => ({ default: (await import('./pages/UserExplorerPage')).UserExplorerPage }));
const RepositoryPage = lazy(async () => ({ default: (await import('./pages/RepositoryPage')).RepositoryPage }));
const SavedPage = lazy(async () => ({ default: (await import('./pages/SavedPage')).SavedPage }));
const SettingsPage = lazy(async () => ({ default: (await import('./pages/SettingsPage')).SettingsPage }));

function RootRoute() {
  const { status, loading, error, refresh } = useAuth();
  if (loading) return <StartupState />;
  if (error) return <AuthError error={error} onRetry={() => void refresh()} />;
  return <Navigate replace to={status?.connected ? '/app/explore' : '/login'} />;
}

function ProtectedRoute() {
  const location = useLocation();
  const { status, loading, error, refresh } = useAuth();
  if (loading) return <StartupState />;
  if (error) return <AuthError error={error} onRetry={() => void refresh()} />;
  if (!status?.connected) {
    return <Navigate replace to="/login" state={{ from: location.pathname + location.search }} />;
  }
  return <Outlet />;
}

function StartupState() {
  return (
    <main className="startup-state" aria-busy="true" aria-live="polite">
      <span className="visually-hidden">Checking your GitHub session.</span>
      <Skeleton variant="block" height="4rem" />
      <Skeleton variant="text" lines={3} />
    </main>
  );
}

function RouteLoadingState() {
  return (
    <section className="route-loading" aria-busy="true" aria-live="polite">
      <span className="visually-hidden">Opening page.</span>
      <Skeleton variant="text" lines={2} />
      <Skeleton variant="block" height="10rem" />
    </section>
  );
}

function Deferred({ children }: { children: ReactNode }) {
  return <Suspense fallback={<RouteLoadingState />}>{children}</Suspense>;
}

function AuthError({ error, onRetry }: { error: Error; onRetry?: () => void }) {
  return (
    <main className="startup-state">
      <Alert
        tone="error"
        title="The GitExplore service is not responding"
        icon={<CircleAlertIcon aria-hidden="true" size={18} />}
        action={onRetry ? (
          <Button variant="outline" leftIcon={<RefreshIcon aria-hidden="true" size={16} />} onClick={onRetry}>
            Try again
          </Button>
        ) : undefined}
      >
        {error.message || 'Authentication status could not be checked. Your session was not treated as signed out.'}
      </Alert>
    </main>
  );
}

function UnknownRoute() {
  return (
    <section className="page-section empty-section">
      <div className="unknown-route-copy">
        <Text size="xs" color="$mutedForeground">Unknown route</Text>
        <Heading size="h1">That path is not part of the atlas.</Heading>
        <Text color="$mutedForeground">Return to Explore and choose a GitHub account.</Text>
        <Link className="text-link" to="/app/explore">Open Explore</Link>
      </div>
    </section>
  );
}

export function App() {
  return (
    <Routes>
      <Route path="/" element={<RootRoute />} />
      <Route path="/login" element={<Deferred><LoginPage /></Deferred>} />
      <Route element={<ProtectedRoute />}>
        <Route path="/app" element={<Deferred><AppShell /></Deferred>}>
          <Route index element={<Navigate replace to="explore" />} />
          <Route path="explore" element={<Deferred><ExploreStartPage /></Deferred>} />
          <Route path="explore/:login" element={<Deferred><UserExplorerPage /></Deferred>} />
          <Route path="repository/:owner/:repo" element={<Deferred><RepositoryPage /></Deferred>} />
          <Route path="saved" element={<Deferred><SavedPage /></Deferred>} />
          <Route path="settings" element={<Deferred><SettingsPage /></Deferred>} />
          <Route path="bookmarks" element={<Navigate replace to="/app/saved?view=bookmarks" />} />
          <Route path="categories" element={<Navigate replace to="/app/saved?view=collections" />} />
          <Route path="explore/snapshots" element={<Navigate replace to="/app/saved?view=history" />} />
          <Route path="sync" element={<Navigate replace to="/app/settings" />} />
          <Route path="*" element={<UnknownRoute />} />
        </Route>
      </Route>
      <Route path="*" element={<RootRoute />} />
    </Routes>
  );
}
