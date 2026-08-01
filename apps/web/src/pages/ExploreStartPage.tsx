import { useQuery } from '@tanstack/react-query';
import type { Bookmark } from '@gitexplore/api-client';
import { type FormEvent, useMemo, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Alert, Avatar, Badge, Button, Heading, SearchField, Skeleton, Surface, Text } from 'strawn';
import { ArrowRightIcon, CircleAlertIcon, GitHubIcon, UsersIcon } from 'strawn-icons';

import { api } from '../api';
import { useAuth } from '../auth';
import { buildExploreHref, isLikelyGitHubLogin, normalizeLoginInput } from '../lib/graph-navigation';
import { useDocumentTitle } from '../useDocumentTitle';

export function ExploreStartPage() {
  useDocumentTitle('Explore');
  const navigate = useNavigate();
  const { status } = useAuth();
  const accountLogin = status?.account?.login ?? '';
  const [search, setSearch] = useState(accountLogin);
  const [validationError, setValidationError] = useState('');

  const bookmarksQuery = useQuery({
    queryKey: ['bookmarks'],
    queryFn: () => api.getBookmarks(),
    staleTime: 60_000,
    retry: false,
  });
  const savedPeople = useMemo(
    () => (bookmarksQuery.data ?? []).filter((bookmark): bookmark is Bookmark & { target: { GitHubUser: { login: string } } } => 'GitHubUser' in bookmark.target).slice(0, 8),
    [bookmarksQuery.data],
  );

  function openGraph(rawLogin: string) {
    const login = normalizeLoginInput(rawLogin);
    if (!isLikelyGitHubLogin(login)) {
      setValidationError('Enter a GitHub username, @handle, or github.com profile URL.');
      return;
    }
    setValidationError('');
    navigate(buildExploreHref(login));
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    openGraph(search);
  }

  return (
    <div className="page-stack explore-start">
      <header className="page-heading">
        <div>
          <Text size="xs" color="$mutedForeground">Graph explorer</Text>
          <Heading size="h1">Whose GitHub world<br />should we walk?</Heading>
        </div>
        <Text color="$mutedForeground">
          Start with a person. Every follower, maintainer, and repository becomes a branch you can follow without losing the path behind you.
        </Text>
      </header>

      <Surface as="section" className="search-stage" tone="default" radius="lg" padding="lg" aria-labelledby="search-title">
        <div className="search-stage-copy">
          <Heading id="search-title" size="h2">Open a public profile</Heading>
          <Text size="sm" color="$mutedForeground">Handles and full github.com profile URLs both work.</Text>
        </div>
        <form className="profile-search" onSubmit={submit} noValidate>
          <SearchField
            label="GitHub username"
            value={search}
            onChange={(event) => setSearch(event.currentTarget.value)}
            onClear={search ? () => setSearch('') : undefined}
            placeholder="octocat or github.com/octocat"
            autoCapitalize="none"
            autoComplete="off"
            spellCheck={false}
            required
            error={validationError || undefined}
          />
          <Button type="submit" size="lg" rightIcon={<ArrowRightIcon aria-hidden="true" size={17} />}>Open graph</Button>
        </form>
      </Surface>

      <div className="explore-start-grid">
        <section className="start-account" aria-labelledby="your-network-title">
          <div className="section-heading-row">
            <div>
              <Text size="xs" color="$mutedForeground">Connected trailhead</Text>
              <Heading id="your-network-title" size="h2">Your network</Heading>
            </div>
            <GitHubIcon aria-hidden="true" size={22} />
          </div>
          <div className="start-account-row">
            <Avatar
              name={status?.account?.display_name || accountLogin || 'GitHub account'}
              src={accountLogin ? `https://github.com/${encodeURIComponent(accountLogin)}.png?size=112` : undefined}
              size="lg"
            />
            <div><strong>{status?.account?.display_name || accountLogin}</strong><span>@{accountLogin}</span></div>
          </div>
          {accountLogin ? (
            <Link className="secondary-link" to={buildExploreHref(accountLogin)}>
              Walk from here <ArrowRightIcon aria-hidden="true" size={16} />
            </Link>
          ) : null}
        </section>

        <section className="saved-people" aria-labelledby="saved-people-title">
          <div className="section-heading-row">
            <div>
              <Text size="xs" color="$mutedForeground">Known signals</Text>
              <Heading id="saved-people-title" size="h2">Saved people</Heading>
            </div>
            <Badge tone="neutral" leadingIcon={<UsersIcon aria-hidden="true" size={14} />}>{savedPeople.length}</Badge>
          </div>
          {bookmarksQuery.isPending ? (
            <div className="saved-person-list" aria-busy="true"><Skeleton height="3.5rem" /><Skeleton height="3.5rem" /></div>
          ) : bookmarksQuery.isError ? (
            <Alert tone="error" title="Saved people are unavailable" icon={<CircleAlertIcon aria-hidden="true" size={17} />}>
              {bookmarksQuery.error instanceof Error ? bookmarksQuery.error.message : 'The request failed.'}
            </Alert>
          ) : savedPeople.length ? (
            <ul className="saved-person-list">
              {savedPeople.map((bookmark) => (
                <li key={bookmark.id}>
                  <Link to={buildExploreHref(bookmark.target.GitHubUser.login)}>
                    <Avatar name={bookmark.target.GitHubUser.login} size="sm" />
                    <span>@{bookmark.target.GitHubUser.login}</span>
                    <ArrowRightIcon aria-hidden="true" size={15} />
                  </Link>
                </li>
              ))}
            </ul>
          ) : (
            <div className="inline-empty"><Text size="sm" color="$mutedForeground">Save a person and they will become a restart point here.</Text></div>
          )}
        </section>
      </div>
    </div>
  );
}
