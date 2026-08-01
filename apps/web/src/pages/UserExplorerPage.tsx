import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { GitExploreApiError, type RepositoryCandidate, type UserNeighborhood } from '@gitexplore/api-client';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import { Alert, Avatar, Badge, Button, Heading, Skeleton, Tabs, Text } from 'strawn';
import { ArrowLeftIcon, CircleAlertIcon, ExternalLinkIcon, RefreshIcon, UsersIcon } from 'strawn-icons';

import { api } from '../api';
import { ExpeditionProgress } from '../components/ExpeditionProgress';
import { GraphTrail } from '../components/GraphTrail';
import { PersonList } from '../components/PersonList';
import { RepositoryCandidateCard } from '../components/RepositoryCandidateCard';
import { UserInsights } from '../components/UserInsights';
import { isLikelyGitHubLogin, normalizeConnectionDirection, normalizeLoginInput, normalizeTrail, setConnectionDirection } from '../lib/graph-navigation';
import { cacheLabel, compactNumber } from '../lib/format';
import { useDocumentTitle } from '../useDocumentTitle';

const neighborhoodLimit = 36;

export function neighborhoodNeedsExpansion(value: UserNeighborhood | null) {
  return value === null || value.lastFetchedAt === null;
}

export function expansionTargetsLogin(requestedLogin: string | undefined, currentLogin: string) {
  return Boolean(requestedLogin) && requestedLogin?.toLowerCase() === currentLogin.toLowerCase();
}

function neighborhoodKey(login: string) {
  return ['user-neighborhood', login.toLowerCase(), neighborhoodLimit] as const;
}

export function UserExplorerPage() {
  const { login: rawLogin = '' } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const attemptedAutoExpansions = useRef(new Set<string>());
  const [visibleRepositories, setVisibleRepositories] = useState(8);
  const [saveErrors, setSaveErrors] = useState<Record<string, string>>({});

  const login = normalizeLoginInput(rawLogin);
  const validLogin = isLikelyGitHubLogin(login);
  const trail = useMemo(() => normalizeTrail(searchParams.get('trail'), login), [login, searchParams]);
  const direction = normalizeConnectionDirection(searchParams.get('direction'));
  useDocumentTitle(validLogin ? `@${login}` : 'Explore');

  const neighborhoodQuery = useQuery({
    queryKey: neighborhoodKey(login),
    queryFn: async (): Promise<UserNeighborhood | null> => {
      try {
        return await api.getNeighborhood(login, neighborhoodLimit);
      } catch (error) {
        if (error instanceof GitExploreApiError && error.code === 'NOT_FOUND') return null;
        throw error;
      }
    },
    enabled: validLogin,
    staleTime: 60_000,
    gcTime: 30 * 60_000,
    retry: false,
  });

  const expansion = useMutation({
    mutationFn: ({ requestedLogin }: { requestedLogin: string; automatic: boolean }) => api.expandUser(requestedLogin, neighborhoodLimit),
    retry: false,
    onSuccess: async (neighborhood, variables) => {
      queryClient.setQueryData(neighborhoodKey(variables.requestedLogin), neighborhood);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['user-insights', variables.requestedLogin.toLowerCase()] }),
        queryClient.invalidateQueries({ queryKey: ['github-rate-limit'] }),
      ]);
    },
  });

  const neighborhood = neighborhoodQuery.data ?? null;
  const expansionIsForCurrentLogin = expansionTargetsLogin(expansion.variables?.requestedLogin, login);
  const currentExpansionPending = expansion.isPending && expansionIsForCurrentLogin;
  const currentExpansionError = expansion.isError && expansionIsForCurrentLogin;

  useEffect(() => {
    setVisibleRepositories(8);
  }, [login]);

  useEffect(() => {
    const key = login.toLowerCase();
    if (!validLogin || !neighborhoodQuery.isSuccess || !neighborhoodNeedsExpansion(neighborhoodQuery.data) || attemptedAutoExpansions.current.has(key)) return;
    attemptedAutoExpansions.current.add(key);
    expansion.mutate({ requestedLogin: login, automatic: true });
  }, [expansion, login, neighborhoodQuery.data, neighborhoodQuery.isSuccess, validLogin]);

  const insightsQuery = useQuery({
    queryKey: ['user-insights', login.toLowerCase(), 12],
    queryFn: () => api.getUserInsights(login, 12),
    enabled: validLogin && Boolean(neighborhood),
    staleTime: 10 * 60_000,
    gcTime: 60 * 60_000,
    retry: false,
  });

  const saveMutation = useMutation({
    mutationFn: (fullName: string) => api.saveRepository(fullName, [], null),
    retry: false,
    onSuccess: (_saved, fullName) => {
      queryClient.setQueriesData<UserNeighborhood | null>({ queryKey: ['user-neighborhood'] }, (current) => current ? ({
        ...current,
        repositories: current.repositories.map((candidate) => candidate.repository.fullName === fullName ? { ...candidate, saved: true } : candidate),
      }) : current);
      void queryClient.invalidateQueries({ queryKey: ['bookmarks'] });
    },
  });

  const partialCollections = neighborhood ? [
    !neighborhood.coverage.followersComplete && 'followers',
    !neighborhood.coverage.followingComplete && 'following',
    !neighborhood.coverage.starredRepositoriesComplete && 'starred repositories',
    !neighborhood.coverage.repositoriesComplete && 'owned repositories',
  ].filter((label): label is string => Boolean(label)) : [];

  async function saveCandidate(candidate: RepositoryCandidate) {
    const fullName = candidate.repository.fullName;
    setSaveErrors((current) => ({ ...current, [fullName]: '' }));
    try {
      await saveMutation.mutateAsync(fullName);
    } catch (error) {
      setSaveErrors((current) => ({ ...current, [fullName]: error instanceof Error ? error.message : 'The repository could not be saved.' }));
    }
  }

  if (!validLogin) {
    return (
      <Alert tone="error" title="That GitHub username is not valid" icon={<CircleAlertIcon aria-hidden="true" size={18} />}>
        Use a username, @handle, or github.com profile URL from <Link to="/app/explore">Explore</Link>.
      </Alert>
    );
  }

  if (neighborhoodQuery.isPending || (neighborhoodQuery.isSuccess && !neighborhood && !currentExpansionError)) {
    return (
      <div className="page-stack" aria-busy="true" aria-live="polite">
        <GraphTrail trail={trail} direction={direction} />
        <section className="node-loading">
          <Skeleton variant="avatar" width="5rem" height="5rem" />
          <div><Skeleton variant="text" lines={2} /><Text size="sm" color="$mutedForeground">{currentExpansionPending ? `Mapping @${login} for the first time.` : 'Checking the shared graph cache.'}</Text></div>
        </section>
      </div>
    );
  }

  if (neighborhoodQuery.isError || (!neighborhood && currentExpansionError)) {
    const error = neighborhoodQuery.error ?? (currentExpansionError ? expansion.error : null);
    const retryMissingExpansion = !neighborhood && currentExpansionError;
    return (
      <Alert
        tone="error"
        title={`We could not open @${login}`}
        icon={<CircleAlertIcon aria-hidden="true" size={18} />}
        action={
          <Button
            variant="outline"
            onClick={() => retryMissingExpansion
              ? expansion.mutate({ requestedLogin: login, automatic: true })
              : void neighborhoodQuery.refetch()}
          >
            Try again
          </Button>
        }
      >
        {error instanceof Error ? error.message : 'The graph request failed.'}
      </Alert>
    );
  }

  if (!neighborhood) return null;

  const expandingCurrent = currentExpansionPending;
  const displayedRepositories = neighborhood.repositories.slice(0, visibleRepositories);

  return (
    <div className="page-stack">
      <div className="utility-row">
        <Link className="quiet-link" to="/app/explore"><ArrowLeftIcon aria-hidden="true" size={15} /> Find another person</Link>
        <Badge tone={neighborhood.cacheStatus === 'FRESH' ? 'success' : neighborhood.cacheStatus === 'REFRESH_FAILED' ? 'error' : 'warning'}>
          {cacheLabel(neighborhood.cacheStatus)}
        </Badge>
      </div>
      <GraphTrail trail={trail} direction={direction} />

      <section className="node-profile" aria-labelledby="node-title" aria-busy={expandingCurrent}>
        <div className="node-person">
          <Avatar src={neighborhood.user.avatarUrl ?? undefined} name={neighborhood.user.name || neighborhood.user.login} size="lg" />
          <div>
            <Text size="xs" color="$mutedForeground">Current node</Text>
            <Heading id="node-title" size="h1">{neighborhood.user.name || neighborhood.user.login}</Heading>
            <p className="mono-handle">@{neighborhood.user.login}</p>
            {neighborhood.user.bio ? <Text size="sm" color="$mutedForeground">{neighborhood.user.bio}</Text> : null}
          </div>
        </div>
        <div className="node-actions">
          <a className="secondary-link" href={neighborhood.user.url} target="_blank" rel="noreferrer">
            GitHub <ExternalLinkIcon aria-hidden="true" size={14} /><span className="visually-hidden"> opens in a new tab</span>
          </a>
          <Button
            loading={expandingCurrent}
            leftIcon={<RefreshIcon aria-hidden="true" size={16} />}
            onClick={() => expansion.mutate({ requestedLogin: login, automatic: false })}
          >
            Refresh node
          </Button>
        </div>
        <dl className="node-metrics">
          <div><dt>Followers</dt><dd>{compactNumber(neighborhood.user.followersCount ?? neighborhood.followers.length)}</dd></div>
          <div><dt>Following</dt><dd>{compactNumber(neighborhood.user.followingCount ?? neighborhood.following.length)}</dd></div>
          <div><dt>Repo signals</dt><dd>{compactNumber(neighborhood.repositories.length)}</dd></div>
          <div><dt>Trail depth</dt><dd>{Math.max(0, trail.length - 1)}</dd></div>
        </dl>
      </section>

      {expandingCurrent ? (
        <Alert tone="info" title={expansion.variables?.automatic ? `Mapping @${login}` : `Refreshing @${login}`}>
          Cached context stays visible while public GitHub data is collected.
        </Alert>
      ) : expansion.isError && expansion.variables?.requestedLogin.toLowerCase() === login.toLowerCase() ? (
        <Alert tone="error" title="Refresh failed">{expansion.error.message} Cached results are still shown.</Alert>
      ) : null}
      {partialCollections.length ? (
        <Alert tone="warning" title="This node is partially mapped">
          GitHub capped {partialCollections.join(', ')}. Earlier cached relationships were preserved.
        </Alert>
      ) : null}

      <ExpeditionProgress trailDepth={Math.max(0, trail.length - 1)} repositoryCount={neighborhood.repositories.length} />

      <div className="atlas-grid">
        <aside className="connection-panel" aria-label="Connections">
          <Tabs
            label="Connection direction"
            value={direction}
            onValueChange={(nextDirection) => setSearchParams(setConnectionDirection(searchParams, nextDirection), { replace: true })}
            items={[
              { value: 'followers', label: `Followers ${neighborhood.followers.length}`, content: <PersonList people={neighborhood.followers} trail={trail} direction="followers" /> },
              { value: 'following', label: `Following ${neighborhood.following.length}`, content: <PersonList people={neighborhood.following} trail={trail} direction="following" /> },
            ]}
          />
        </aside>

        <div className="discovery-column">
          {insightsQuery.isPending ? <section className="insight-loading" aria-busy="true"><Skeleton variant="text" lines={4} /></section> : null}
          {insightsQuery.data ? <UserInsights insight={insightsQuery.data} trail={trail} direction={direction} /> : null}
          {insightsQuery.isError ? <Alert tone="error" title="Recent commit activity is unavailable">The cached graph remains available.</Alert> : null}

          <section className="discoveries" aria-labelledby="discoveries-title">
            <div className="section-heading-row discoveries-heading">
              <div>
                <Text size="xs" color="$mutedForeground">Ranked discoveries</Text>
                <Heading id="discoveries-title" size="h2">Repositories with local signal</Heading>
                <Text size="sm" color="$mutedForeground">Nearby endorsement and recent activity matter before raw popularity.</Text>
              </div>
              <Badge tone="neutral" leadingIcon={<UsersIcon aria-hidden="true" size={14} />}>{neighborhood.repositories.length} found</Badge>
            </div>
            {displayedRepositories.length ? (
              <div className="repository-results">
                {displayedRepositories.map((candidate, index) => (
                  <RepositoryCandidateCard
                    key={candidate.repository.fullName}
                    candidate={candidate}
                    rank={index + 1}
                    trail={trail}
                    direction={direction}
                    saving={saveMutation.isPending && saveMutation.variables === candidate.repository.fullName}
                    error={saveErrors[candidate.repository.fullName]}
                    onSave={() => void saveCandidate(candidate)}
                  />
                ))}
                {visibleRepositories < neighborhood.repositories.length ? (
                  <Button variant="outline" onClick={() => setVisibleRepositories((count) => count + 8)}>
                    Show {Math.min(8, neighborhood.repositories.length - visibleRepositories)} more
                  </Button>
                ) : null}
              </div>
            ) : (
              <div className="inline-empty"><Text size="sm" color="$mutedForeground">No repository signals yet. Refresh this node or continue through a connection.</Text></div>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
