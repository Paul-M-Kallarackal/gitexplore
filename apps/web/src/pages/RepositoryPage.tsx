import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import { Alert, Avatar, Badge, Button, Heading, Skeleton, Text } from 'strawn';
import { ArrowLeftIcon, ArrowRightIcon, BookmarkIcon, CheckIcon, CircleAlertIcon, ExternalLinkIcon, GitHubIcon } from 'strawn-icons';

import { api } from '../api';
import { GraphTrail } from '../components/GraphTrail';
import { buildExploreHref, normalizeConnectionDirection, parseTrail, type ConnectionDirection } from '../lib/graph-navigation';
import { cacheLabel, compactNumber, formatTimestamp } from '../lib/format';
import { useOnboarding } from '../onboarding';
import { useDocumentTitle } from '../useDocumentTitle';

const ownerPattern = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/;
const repositoryPattern = /^[A-Za-z0-9._-]{1,100}$/;

export function RepositoryPage() {
  const { owner = '', repo = '' } = useParams();
  const [searchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const { refreshProgress } = useOnboarding();
  const fullName = `${owner}/${repo}`;
  const valid = ownerPattern.test(owner) && repositoryPattern.test(repo);
  const trail = useMemo(() => parseTrail(searchParams.get('trail')), [searchParams]);
  const direction: ConnectionDirection | undefined = searchParams.has('direction')
    ? normalizeConnectionDirection(searchParams.get('direction'))
    : undefined;
  useDocumentTitle(valid ? fullName : 'Repository');

  const insightsQuery = useQuery({
    queryKey: ['repository-insights', fullName.toLowerCase(), 16],
    queryFn: () => api.getRepositoryInsights(fullName, 16),
    enabled: valid,
    staleTime: 10 * 60_000,
    gcTime: 60 * 60_000,
    retry: false,
  });
  const bookmarksQuery = useQuery({
    queryKey: ['bookmarks'],
    queryFn: () => api.getBookmarks(),
    enabled: valid,
    staleTime: 60_000,
    retry: false,
  });
  const saveMutation = useMutation({
    mutationFn: () => api.saveRepository(fullName, [], null),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['bookmarks'] }),
        queryClient.invalidateQueries({ queryKey: ['user-neighborhood'] }),
        refreshProgress(),
      ]);
    },
    retry: false,
  });
  const saved = (bookmarksQuery.data ?? []).some((bookmark) =>
    'GitHubRepository' in bookmark.target && bookmark.target.GitHubRepository.full_name.toLowerCase() === fullName.toLowerCase(),
  );

  if (!valid) {
    return (
      <Alert tone="error" title="That repository name is not valid" icon={<CircleAlertIcon aria-hidden="true" size={18} />}>
        Open a repository from <Link to="/app/explore">Explore</Link> or use an owner/name path.
      </Alert>
    );
  }

  return (
    <div className="page-stack repository-page">
      <div className="utility-row">
        <Link className="quiet-link" to={trail.length ? buildExploreHref(trail.at(-1) ?? owner, trail.slice(0, -1), direction) : '/app/explore'}>
          <ArrowLeftIcon aria-hidden="true" size={15} /> Back to discovery
        </Link>
        {insightsQuery.data ? <Badge tone={insightsQuery.data.sourceComplete ? 'success' : 'warning'}>{cacheLabel(insightsQuery.data.cacheStatus)}</Badge> : null}
      </div>
      {trail.length ? <GraphTrail trail={trail} direction={direction} /> : null}

      <section className="repository-header" aria-labelledby="repository-title">
        <div className="repository-owner-mark">
          <Avatar src={`https://github.com/${encodeURIComponent(owner)}.png?size=112`} name={owner} size="lg" />
          <span className="repo-connector" aria-hidden="true" />
          <GitHubIcon aria-hidden="true" size={24} />
        </div>
        <div className="repository-heading-copy">
          <Text size="xs" color="$mutedForeground">Repository node · {owner}</Text>
          <Heading id="repository-title" size="h1">{repo}</Heading>
          <Text color="$mutedForeground">Inspect the people carrying this work, then continue through their public graphs.</Text>
        </div>
        <div className="repository-actions">
          <a className="secondary-link" href={`https://github.com/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}`} target="_blank" rel="noreferrer">
            Open on GitHub <ExternalLinkIcon aria-hidden="true" size={14} /><span className="visually-hidden"> opens in a new tab</span>
          </a>
          <Button
            loading={saveMutation.isPending}
            disabled={saved}
            leftIcon={saved ? <CheckIcon aria-hidden="true" size={16} /> : <BookmarkIcon aria-hidden="true" size={16} />}
            onClick={() => saveMutation.mutate()}
          >
            {saved ? 'Saved' : 'Save repository'}
          </Button>
        </div>
      </section>

      {saveMutation.isError ? <Alert tone="error" title="Repository was not saved">{saveMutation.error.message}</Alert> : null}
      {saveMutation.isSuccess ? <span className="visually-hidden" role="status">{fullName} saved.</span> : null}

      <div className="repository-insight-grid">
        <section className="contributors" aria-labelledby="contributors-title">
          <div className="section-heading-row">
            <div>
              <Text size="xs" color="$mutedForeground">Contributor signal</Text>
              <Heading id="contributors-title" size="h2">People carrying the work</Heading>
            </div>
            {insightsQuery.data ? <Badge tone="neutral">{insightsQuery.data.contributors.length} people</Badge> : null}
          </div>

          {insightsQuery.isPending ? (
            <div className="contributor-loading" aria-busy="true" aria-live="polite"><Skeleton height="4.5rem" /><Skeleton height="4.5rem" /><Skeleton height="4.5rem" /></div>
          ) : insightsQuery.isError ? (
            <Alert
              tone="error"
              title="Contributor activity is unavailable"
              action={<Button variant="outline" onClick={() => void insightsQuery.refetch()}>Try again</Button>}
            >
              {insightsQuery.error instanceof Error ? insightsQuery.error.message : 'The request failed.'}
            </Alert>
          ) : insightsQuery.data?.contributors.length ? (
            <ol className="contributor-list">
              {insightsQuery.data.contributors.map((contributor, index) => (
                <li key={contributor.githubId || contributor.login}>
                  <Link to={buildExploreHref(contributor.login, trail, direction)}>
                    <span className="contributor-rank">{String(index + 1).padStart(2, '0')}</span>
                    <Avatar src={contributor.avatarUrl ?? undefined} name={contributor.login} size="md" />
                    <span className="contributor-copy"><strong>@{contributor.login}</strong><small>{compactNumber(contributor.contributions)} attributed commits</small></span>
                    <ArrowRightIcon aria-hidden="true" size={16} />
                  </Link>
                </li>
              ))}
            </ol>
          ) : (
            <div className="inline-empty"><Text size="sm" color="$mutedForeground">No contributor records are cached for this repository.</Text></div>
          )}
        </section>

        <aside className="field-notes" aria-labelledby="field-notes-title">
          <Text size="xs" color="$mutedForeground">Field notes</Text>
          <Heading id="field-notes-title" size="h2">How to read this node</Heading>
          <dl>
            <div><dt>Contributor order</dt><dd>GitHub-attributed commit totals across public history.</dd></div>
            <div><dt>Cache policy</dt><dd>Stale results remain visible during one shared refresh.</dd></div>
            {insightsQuery.data ? <div><dt>Last fetched</dt><dd>{formatTimestamp(insightsQuery.data.lastFetchedAt)}</dd></div> : null}
          </dl>
          <Link className="thread-link" to={buildExploreHref(owner, trail, direction)}>
            Explore @{owner} <ArrowRightIcon aria-hidden="true" size={16} />
          </Link>
        </aside>
      </div>
    </div>
  );
}
