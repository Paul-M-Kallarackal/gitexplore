import { Link, Navigate, useLocation } from 'react-router-dom';
import { Alert, Button, Heading, Surface, Text } from 'strawn';
import { ArrowRightIcon, BookmarkIcon, CircleAlertIcon, GitHubIcon, UsersIcon } from 'strawn-icons';

import { api } from '../api';
import { useAuth } from '../auth';
import { GitExploreWordmark } from '../components/GitExploreWordmark';
import { useDocumentTitle } from '../useDocumentTitle';

export function LoginPage() {
  useDocumentTitle('Sign in');
  const { status, loading, error, refresh } = useAuth();
  const location = useLocation();
  const requestedPath = (location.state as { from?: string } | null)?.from;
  const safePath = requestedPath?.startsWith('/app') ? requestedPath : '/app/explore';
  const returnUrl = `${window.location.origin}${safePath}`;

  if (!loading && !error && status?.connected) return <Navigate replace to={safePath} />;

  return (
    <main className="login-page">
      <header className="login-header">
        <Link className="wordmark" to="/" aria-label="GitExplore home">
          <GitExploreWordmark />
        </Link>
        <span>Public graph · private saves</span>
      </header>

      <div className="login-composition">
        <section className="login-thesis" aria-labelledby="login-title">
          <div className="login-thread" aria-hidden="true">
            <span /><span /><span /><span />
          </div>
          <div className="login-hero-copy">
            <Text size="xs" color="$mutedForeground">A field notebook for open source</Text>
            <Heading id="login-title" size="h1">Follow people.<br />Find the work between the stars.</Heading>
            <Text color="$mutedForeground">
              Walk outward through followers and maintainers. GitExplore keeps the route visible, so an obscure repository never becomes a lost tab.
            </Text>
          </div>
          <ol className="login-steps">
            <li><span>01</span><UsersIcon aria-hidden="true" size={18} /><strong>Choose a person</strong></li>
            <li><span>02</span><ArrowRightIcon aria-hidden="true" size={18} /><strong>Follow the signal</strong></li>
            <li><span>03</span><BookmarkIcon aria-hidden="true" size={18} /><strong>Save the find</strong></li>
          </ol>
        </section>

        <Surface as="section" className="login-connect" tone="default" radius="lg" padding="lg" aria-labelledby="connect-title">
          <div>
            <Text size="xs" color="$mutedForeground">Your trailhead</Text>
            <Heading id="connect-title" size="h2">Connect GitHub</Heading>
            <Text color="$mutedForeground">
              Your browser receives a secure session cookie. Public graph facts can be reused; bookmarks and collections remain yours.
            </Text>
          </div>

          {error ? (
            <Alert
              tone="error"
              title="GitExplore could not check your session"
              icon={<CircleAlertIcon aria-hidden="true" size={18} />}
              action={<Button variant="outline" onClick={() => void refresh()}>Retry</Button>}
            >
              {error.message}
            </Alert>
          ) : null}

          <div className="login-action">
            <a
              className="primary-link github-connect-link"
              href={api.startBrowserOAuth(returnUrl)}
            >
              <span><GitHubIcon aria-hidden="true" size={18} /> Continue with GitHub</span>
              <ArrowRightIcon aria-hidden="true" size={17} />
            </a>
            <Text size="xs" color="$mutedForeground">Read-only GitHub access. Sign out whenever you want.</Text>
          </div>
        </Surface>
      </div>
    </main>
  );
}
