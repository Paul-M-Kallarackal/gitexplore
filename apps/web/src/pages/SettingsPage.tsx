import type { DiscoveryWarmup, DiscoveryWarmupStatus } from '@gitexplore/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Alert, Avatar, Badge, Button, Heading, Progress, Skeleton, Text } from 'strawn';
import { CircleAlertIcon, ClockIcon, DatabaseIcon, LogOutIcon, MapPinIcon, RefreshIcon } from 'strawn-icons';

import { api } from '../api';
import { useAuth, useLogout } from '../auth';
import { formatTimestamp } from '../lib/format';
import { useOnboarding } from '../onboarding';
import { useDocumentTitle } from '../useDocumentTitle';

const activeWarmupStatuses: DiscoveryWarmupStatus[] = ['QUEUED', 'RUNNING'];

function isWarmupActive(status: DiscoveryWarmupStatus | undefined) {
  return status ? activeWarmupStatuses.includes(status) : false;
}

function warmupStatus(status: DiscoveryWarmupStatus | undefined) {
  switch (status) {
    case 'QUEUED':
      return { label: 'Preparing map', badge: 'Queued', tone: 'info' as const };
    case 'RUNNING':
      return { label: 'Mapping network', badge: 'Active', tone: 'info' as const };
    case 'COMPLETED':
      return { label: 'Map is warm', badge: 'Complete', tone: 'success' as const };
    case 'RESERVE_PROTECTED':
      return { label: 'Waiting for GitHub reset', badge: 'Reserve held', tone: 'warning' as const };
    case 'FAILED':
      return { label: 'Mapping paused', badge: 'Needs attention', tone: 'error' as const };
    default:
      return { label: 'Ready to map', badge: 'Not started', tone: 'neutral' as const };
  }
}

function warmupDetail(warmup: DiscoveryWarmup | null | undefined) {
  if (!warmup) return 'Start with your connected GitHub account.';
  if (warmup.status === 'RUNNING' && warmup.currentLogin) {
    return `Mapping @${warmup.currentLogin}, updated ${formatTimestamp(warmup.updatedAt)}`;
  }
  if (warmup.status === 'QUEUED') return `Starting from @${warmup.seedLogin}`;
  if (warmup.status === 'RESERVE_PROTECTED') return `Resumes after ${formatTimestamp(warmup.resetAt)}`;
  if (warmup.status === 'COMPLETED') return `Completed ${formatTimestamp(warmup.completedAt)}`;
  return `Updated ${formatTimestamp(warmup.updatedAt)}`;
}

function warmupActionLabel(
  warmup: DiscoveryWarmup | null | undefined,
  waitingForReset: boolean,
) {
  switch (warmup?.status) {
    case 'QUEUED':
      return 'Mapping queued';
    case 'RUNNING':
      return 'Mapping network';
    case 'RESERVE_PROTECTED':
      return waitingForReset ? 'Available after reset' : 'Continue mapping';
    case 'FAILED':
      return 'Retry mapping';
    case 'COMPLETED':
      return 'Map warmed';
    default:
      return 'Start mapping';
  }
}

export function SettingsPage() {
  useDocumentTitle('Settings');
  const { status } = useAuth();
  const logout = useLogout();
  const queryClient = useQueryClient();
  const { progress: onboarding, restart, restartPending, error: onboardingError } = useOnboarding();

  const warmupQuery = useQuery({
    queryKey: ['discovery-warmup'],
    queryFn: () => api.getDiscoveryWarmup(),
    retry: false,
    refetchInterval: (query) => isWarmupActive(query.state.data?.status) ? 2_500 : false,
  });
  const rateQuery = useQuery({ queryKey: ['github-rate-limit'], queryFn: () => api.getRateLimit(), staleTime: 60_000, retry: 1 });
  const warmupMutation = useMutation({
    mutationFn: () => api.startDiscoveryWarmup(),
    onSuccess: async (warmup) => {
      queryClient.setQueryData(['discovery-warmup'], warmup);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['github-rate-limit'] }),
        queryClient.invalidateQueries({ queryKey: ['user-neighborhood'] }),
      ]);
    },
    retry: false,
  });
  const logoutMutation = useMutation({ mutationFn: logout, retry: false });
  const warmup = warmupQuery.data;
  const statusPresentation = warmupStatus(warmup?.status);
  const warmupTotal = warmup
    ? Math.max(warmup.discoveredUsers, warmup.expandedUsers + warmup.pendingUsers, 1)
    : 1;
  const warmupWaitingForReset = warmup?.status === 'RESERVE_PROTECTED'
    && (!warmup.resetAt || new Date(warmup.resetAt).getTime() > Date.now());

  return (
    <div className="page-stack settings-page">
      <header className="page-heading compact">
        <div><Text size="xs" color="$mutedForeground">Account and data</Text><Heading size="h1">Settings</Heading></div>
        <Text color="$mutedForeground">Session, GitHub request budget, and discovery mapping.</Text>
      </header>

      <section className="settings-section" aria-labelledby="connection-title">
        <div className="settings-heading">
          <span className="settings-index">01</span>
          <div><Heading id="connection-title" size="h2">Connection</Heading><Text size="sm" color="$mutedForeground">The GitHub identity attached to this private app session.</Text></div>
        </div>
        <div className="connection-settings">
          <div className="account-large">
            <Avatar
              name={status?.account?.display_name || status?.account?.login || 'GitHub account'}
              src={status?.account?.login ? `https://github.com/${encodeURIComponent(status.account.login)}.png?size=112` : undefined}
              size="lg"
            />
            <span><strong>{status?.account?.display_name || status?.account?.login}</strong><small>@{status?.account?.login}</small></span>
            <Badge tone="success">Connected</Badge>
          </div>
          <Button
            variant="outline"
            loading={logoutMutation.isPending}
            leftIcon={<LogOutIcon aria-hidden="true" size={16} />}
            onClick={() => logoutMutation.mutate()}
          >
            Sign out
          </Button>
        </div>
        {logoutMutation.isError ? <Alert tone="error" title="Sign out failed">{logoutMutation.error.message}</Alert> : null}
      </section>

      <section className="settings-section" aria-labelledby="budget-title">
        <div className="settings-heading">
          <span className="settings-index">02</span>
          <div><Heading id="budget-title" size="h2">GitHub request budget</Heading><Text size="sm" color="$mutedForeground">The authenticated REST core window normally resets hourly.</Text></div>
        </div>
        {rateQuery.isPending ? <Skeleton height="6rem" /> : null}
        {rateQuery.isError ? (
          <Alert tone="error" title="Rate limit is unavailable" action={<Button variant="outline" onClick={() => void rateQuery.refetch()}>Retry</Button>}>
            {rateQuery.error instanceof Error ? rateQuery.error.message : 'The request failed.'}
          </Alert>
        ) : null}
        {rateQuery.data ? (
          <div className="budget-settings">
            <Progress label="Requests remaining" value={rateQuery.data.remaining} max={rateQuery.data.limit} />
            <dl>
              <div><dt>Remaining</dt><dd>{rateQuery.data.remaining.toLocaleString()}</dd></div>
              <div><dt>Used</dt><dd>{rateQuery.data.used.toLocaleString()}</dd></div>
              <div><dt>Resets</dt><dd>{formatTimestamp(rateQuery.data.resetAt)}</dd></div>
            </dl>
          </div>
        ) : null}
      </section>

      <section className="settings-section" aria-labelledby="warmup-title">
        <div className="settings-heading">
          <span className="settings-index">03</span>
          <div>
            <Heading id="warmup-title" size="h2">Discovery map</Heading>
            <Text size="sm" color="$mutedForeground">Maps outward through followers and following until it reaches a protected 1,000-request reserve, then can continue after GitHub's hourly reset.</Text>
          </div>
        </div>
        <div className="warmup-panel">
          {warmupQuery.isPending ? <Skeleton height="8.5rem" /> : null}
          {warmupQuery.isError ? (
            <Alert tone="error" title="Discovery status is unavailable" action={<Button variant="outline" onClick={() => void warmupQuery.refetch()}>Retry</Button>}>
              {warmupQuery.error instanceof Error ? warmupQuery.error.message : 'The request failed.'}
            </Alert>
          ) : null}
          {!warmupQuery.isPending && !warmupQuery.isError ? (
            <>
              <div className="warmup-settings">
                <div className="warmup-state" role="status" aria-live="polite">
                  <DatabaseIcon aria-hidden="true" size={22} />
                  <span>
                    <span className="warmup-status-line"><strong>{statusPresentation.label}</strong><Badge tone={statusPresentation.tone}>{statusPresentation.badge}</Badge></span>
                    <small><ClockIcon aria-hidden="true" size={13} /> {warmupDetail(warmup)}</small>
                  </span>
                </div>
                <div className="warmup-actions">
                  <Button variant="ghost" leftIcon={<RefreshIcon aria-hidden="true" size={16} />} onClick={() => void warmupQuery.refetch()}>Refresh status</Button>
                  <Button
                    loading={warmupMutation.isPending}
                    disabled={isWarmupActive(warmup?.status) || warmupWaitingForReset || warmup?.status === 'COMPLETED'}
                    onClick={() => warmupMutation.mutate()}
                  >
                    {warmupActionLabel(warmup, warmupWaitingForReset)}
                  </Button>
                </div>
              </div>
              {warmup ? (
                <>
                  <div className="warmup-progress">
                    <Progress
                      label="Network mapped"
                      value={warmup.expandedUsers}
                      max={warmupTotal}
                      indeterminate={warmup.status === 'QUEUED' && warmup.expandedUsers === 0}
                    />
                  </div>
                  <dl className="warmup-summary">
                    <div><dt>Expanded</dt><dd>{warmup.expandedUsers.toLocaleString()}</dd></div>
                    <div><dt>Discovered</dt><dd>{warmup.discoveredUsers.toLocaleString()}</dd></div>
                    <div><dt>Pending</dt><dd>{warmup.pendingUsers.toLocaleString()}</dd></div>
                    <div><dt>REST left</dt><dd>{warmup.remainingRequests?.toLocaleString() ?? '\u2014'}</dd></div>
                  </dl>
                </>
              ) : null}
              {warmup?.status === 'RESERVE_PROTECTED' ? (
                <Alert tone="warning" title="The request reserve is protected">
                  The current frontier is saved. Continue mapping after the hourly budget resets {formatTimestamp(warmup.resetAt)}.
                </Alert>
              ) : null}
              {warmup?.frontierTruncated ? (
                <Alert tone="info" title="This frontier was trimmed">
                  Some distant connections were skipped to keep this map bounded and responsive.
                </Alert>
              ) : null}
              {warmup?.lastError ? (
                <Alert tone={warmup.status === 'FAILED' ? 'error' : 'warning'} title="Discovery mapping reported an error" icon={<CircleAlertIcon aria-hidden="true" size={17} />}>
                  {warmup.lastError}
                </Alert>
              ) : null}
            </>
          ) : null}
          {warmupMutation.isError ? (
            <Alert tone="error" title="Mapping could not start" icon={<CircleAlertIcon aria-hidden="true" size={17} />}>
              {warmupMutation.error instanceof Error ? warmupMutation.error.message : 'The request failed.'}
            </Alert>
          ) : null}
        </div>
      </section>

      <section className="settings-section" aria-labelledby="onboarding-settings-title">
        <div className="settings-heading">
          <span className="settings-index">04</span>
          <div>
            <Heading id="onboarding-settings-title" size="h2">First-value guide</Heading>
            <Text size="sm" color="$mutedForeground">Replay the three-step trail through a person, a connection, and a private repository save.</Text>
          </div>
        </div>
        <div className="onboarding-settings-row">
          <span><MapPinIcon aria-hidden="true" size={20} /><span><strong>{onboarding?.status === 'COMPLETED' ? 'Onboarding complete' : onboarding?.status === 'DISMISSED' ? 'Onboarding skipped' : 'Onboarding available'}</strong><small>Restarting creates a fresh activation window without changing saved data.</small></span></span>
          <Button
            variant="outline"
            loading={restartPending}
            onClick={() => void restart().then(() => window.location.assign('/app/explore'))}
          >
            Replay onboarding
          </Button>
        </div>
        {onboardingError ? <Alert tone="warning" title="Onboarding could not be updated">{onboardingError.message}</Alert> : null}
      </section>

    </div>
  );
}
