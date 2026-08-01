import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { Alert, Badge, Button, Heading, Progress, Skeleton, Surface, Text } from 'strawn';
import {
  ArrowRightIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  CircleAlertIcon,
  GlobeIcon,
  MapPinIcon,
} from 'strawn-icons';

import { api } from '../api';
import { useAuth } from '../auth';
import { buildExploreHref } from '../lib/graph-navigation';
import { formatTimestamp } from '../lib/format';
import { onboardingQueryKey, type OnboardingStep, useOnboarding } from '../onboarding';

const steps: Array<{ key: OnboardingStep; label: string; detail: string }> = [
  { key: 'trailhead', label: 'Open a trailhead', detail: 'Start from a real GitHub profile.' },
  { key: 'connection', label: 'Follow one connection', detail: 'Walk through a follower or maintainer.' },
  { key: 'repository', label: 'Save a repository', detail: 'Keep one ranked find in your private notebook.' },
];

function stepComplete(key: OnboardingStep, progress: NonNullable<ReturnType<typeof useOnboarding>['progress']>) {
  if (key === 'trailhead') return progress.openedTrailhead;
  if (key === 'connection') return progress.followedConnection;
  return progress.savedRepository;
}

function CurrentStepAction({ step, login }: { step: OnboardingStep | null; login: string }) {
  const location = useLocation();
  const onUserPage = /^\/app\/explore\/[^/]+/.test(location.pathname);
  if (step === 'trailhead') {
    return (
      <Link className="primary-link onboarding-action" to={buildExploreHref(login)}>
        Start from @{login} <ArrowRightIcon aria-hidden="true" size={16} />
      </Link>
    );
  }
  if (step === 'connection' && onUserPage) {
    return <a className="primary-link onboarding-action" href="#connections">Choose a connection <ArrowRightIcon aria-hidden="true" size={16} /></a>;
  }
  if (step === 'repository' && onUserPage) {
    return <a className="primary-link onboarding-action" href="#discoveries">See ranked repositories <ArrowRightIcon aria-hidden="true" size={16} /></a>;
  }
  return (
    <Link className="primary-link onboarding-action" to={buildExploreHref(login)}>
      Return to your graph <ArrowRightIcon aria-hidden="true" size={16} />
    </Link>
  );
}

function MappingOption() {
  const queryClient = useQueryClient();
  const { refreshProgress } = useOnboarding();
  const warmupQuery = useQuery({
    queryKey: ['discovery-warmup'],
    queryFn: () => api.getDiscoveryWarmup(),
    retry: false,
    refetchInterval: (query) => ['QUEUED', 'RUNNING'].includes(query.state.data?.status ?? '') ? 2_500 : false,
  });
  const rateQuery = useQuery({
    queryKey: ['github-rate-limit'],
    queryFn: () => api.getRateLimit(),
    staleTime: 60_000,
    retry: 1,
  });
  const warmupMutation = useMutation({
    mutationFn: () => api.startDiscoveryWarmup(),
    onSuccess: async (warmup) => {
      queryClient.setQueryData(['discovery-warmup'], warmup);
      await Promise.all([
        refreshProgress(),
        queryClient.invalidateQueries({ queryKey: ['github-rate-limit'] }),
      ]);
    },
    retry: false,
  });
  const warmup = warmupQuery.data;
  const total = warmup ? Math.max(warmup.discoveredUsers, warmup.expandedUsers + warmup.pendingUsers, 1) : 1;

  return (
    <div className="onboarding-mapping">
      <div className="onboarding-mapping-copy">
        <span className="onboarding-mapping-icon" aria-hidden="true"><GlobeIcon size={17} /></span>
        <div>
          <strong>Warm your discovery map</strong>
          <Text size="xs" color="$mutedForeground">
            Optional. Map outward in the background while GitExplore preserves 1,000 GitHub requests.
          </Text>
        </div>
      </div>
      {warmupQuery.isPending ? <Skeleton height="2.75rem" /> : null}
      {!warmupQuery.isPending && !warmup ? (
        <div className="onboarding-mapping-action">
          <span className="numeric-caption">
            {rateQuery.data ? `${rateQuery.data.remaining.toLocaleString()} requests available` : 'The reserve is enforced before every request'}
          </span>
          <Button size="sm" variant="outline" loading={warmupMutation.isPending} onClick={() => warmupMutation.mutate()}>
            Start mapping
          </Button>
        </div>
      ) : null}
      {warmup ? (
        <div className="onboarding-mapping-progress" role="status" aria-live="polite">
          <div><strong>{warmup.status === 'COMPLETED' ? 'Map is warm' : warmup.status === 'RESERVE_PROTECTED' ? 'Reserve protected' : warmup.status === 'FAILED' ? 'Mapping paused' : 'Mapping network'}</strong><Badge tone={warmup.status === 'FAILED' ? 'error' : warmup.status === 'RESERVE_PROTECTED' ? 'warning' : warmup.status === 'COMPLETED' ? 'success' : 'info'}>{warmup.status.replace('_', ' ').toLowerCase()}</Badge></div>
          <Progress label="Discovery mapping progress" value={warmup.expandedUsers} max={total} size="sm" indeterminate={warmup.status === 'QUEUED' && warmup.expandedUsers === 0} />
          {warmup.status === 'RESERVE_PROTECTED' ? <Text size="xs" color="$mutedForeground">Continue after {formatTimestamp(warmup.resetAt)}.</Text> : null}
          {warmup.lastError ? <Text size="xs" color="$mutedForeground">{warmup.lastError}</Text> : null}
        </div>
      ) : null}
      {warmupQuery.isError ? <Text size="xs" color="$mutedForeground">Mapping status is unavailable. Your onboarding can continue.</Text> : null}
      {warmupMutation.isError ? <Alert tone="warning" title="Mapping did not start">{warmupMutation.error.message}</Alert> : null}
    </div>
  );
}

export function OnboardingChecklist() {
  const [skipRequested, setSkipRequested] = useState(false);
  const { status } = useAuth();
  const {
    progress,
    loading,
    error,
    active,
    collapsed,
    currentStep,
    dismissPending,
    setCollapsed,
    dismiss,
    retry,
  } = useOnboarding();
  const login = status?.account?.login ?? '';

  useEffect(() => {
    if (error) setSkipRequested(false);
  }, [error]);

  if (skipRequested) return null;
  if (!active && !error) return null;
  if (loading && !progress) {
    return <section className="onboarding-skeleton" aria-label="Loading onboarding" aria-busy="true"><Skeleton height="5rem" /></section>;
  }
  if (error && !progress) {
    return (
      <Alert
        tone="warning"
        title="Your onboarding guide is unavailable"
        icon={<CircleAlertIcon aria-hidden="true" size={17} />}
        action={<Button variant="outline" onClick={() => void retry()}>Retry</Button>}
      >
        GitExplore is still fully available.
      </Alert>
    );
  }
  if (!progress || !active) return null;

  const completed = steps.filter((step) => stepComplete(step.key, progress)).length;
  if (collapsed) {
    return (
      <button className="onboarding-collapsed" type="button" onClick={() => setCollapsed(false)} aria-expanded="false">
        <MapPinIcon aria-hidden="true" size={17} />
        <span><strong>Your first trail</strong><small>{completed} of 3 complete</small></span>
        <ChevronDownIcon aria-hidden="true" size={16} />
      </button>
    );
  }

  return (
    <Surface as="section" className="onboarding-card" tone="default" radius="lg" padding="none" aria-labelledby="onboarding-title">
      <div className="onboarding-card-layout">
        <Button className="onboarding-collapse" variant="ghost" size="sm" rightIcon={<ChevronUpIcon aria-hidden="true" size={15} />} onClick={() => setCollapsed(true)} aria-expanded="true">
          Collapse
        </Button>
        <div className="onboarding-card-body">
          <div className="onboarding-header">
            <div>
              <Text size="xs" color="$mutedForeground">Field guide · {completed} of 3 complete</Text>
              <Heading id="onboarding-title" size="h2">Your first GitExplore trail</Heading>
              <Text size="sm" color="$mutedForeground">Follow a trusted path to useful work, then keep the find private.</Text>
            </div>
          </div>
          <div className="onboarding-progress">
            <Progress label={`${completed} of 3 onboarding steps complete`} value={completed} max={3} size="sm" />
          </div>
          <ol className="onboarding-steps">
            {steps.map((step, index) => {
              const done = stepComplete(step.key, progress);
              const current = currentStep === step.key;
              return (
                <li key={step.key} className={done ? 'complete' : current ? 'current' : ''} aria-current={current ? 'step' : undefined}>
                  <span className="onboarding-step-marker" aria-hidden="true">{done ? <CheckIcon size={14} /> : index + 1}</span>
                  <span><strong>{step.label}</strong><small>{step.detail}</small></span>
                </li>
              );
            })}
          </ol>
          <div className="onboarding-controls">
            <CurrentStepAction step={currentStep} login={login} />
            <Button
              variant="ghost"
              size="sm"
              loading={dismissPending}
              onClick={() => {
                setSkipRequested(true);
                dismiss();
              }}
            >
              Skip onboarding
            </Button>
          </div>
          <MappingOption />
        </div>
        <picture className="onboarding-art" aria-hidden="true" data-testid="onboarding-artwork">
          <source media="(max-width: 48rem)" srcSet="/images/gitexplore-onboarding-atlas-mobile.webp" />
          <img
            src="/images/gitexplore-onboarding-atlas.webp"
            width="1080"
            height="720"
            alt=""
            decoding="async"
          />
        </picture>
      </div>
    </Surface>
  );
}

export function OnboardingCompletion() {
  const { completionVisible, hideCompletion } = useOnboarding();
  if (!completionVisible) return null;
  return (
    <Alert
      tone="success"
      title="Your first find is saved"
      action={(
        <div className="onboarding-completion-actions">
          <Link className="secondary-link" to="/app/saved" onClick={hideCompletion}>Open Saved</Link>
          <Button variant="ghost" size="sm" onClick={hideCompletion}>Keep exploring</Button>
        </div>
      )}
    >
      The repository is in your private field notebook. Follow another signal whenever you are ready.
    </Alert>
  );
}
