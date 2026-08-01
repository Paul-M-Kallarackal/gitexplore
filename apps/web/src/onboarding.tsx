import type { OnboardingProgress } from '@gitexplore/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

import { api } from './api';

export const onboardingQueryKey = ['onboarding-progress'] as const;

export type OnboardingStep = 'trailhead' | 'connection' | 'repository';

type OnboardingContextValue = {
  progress: OnboardingProgress | undefined;
  loading: boolean;
  error: Error | null;
  active: boolean;
  collapsed: boolean;
  completionVisible: boolean;
  currentStep: OnboardingStep | null;
  dismissPending: boolean;
  restartPending: boolean;
  setCollapsed: (collapsed: boolean) => void;
  dismiss: () => void;
  restart: () => Promise<OnboardingProgress>;
  retry: () => Promise<unknown>;
  refreshProgress: () => Promise<unknown>;
  hideCompletion: () => void;
};

const unavailableOnboarding: OnboardingContextValue = {
  progress: undefined,
  loading: false,
  error: null,
  active: false,
  collapsed: false,
  completionVisible: false,
  currentStep: null,
  dismissPending: false,
  restartPending: false,
  setCollapsed: () => undefined,
  dismiss: () => undefined,
  restart: async () => { throw new Error('Onboarding is unavailable outside the app shell.'); },
  retry: async () => undefined,
  refreshProgress: async () => undefined,
  hideCompletion: () => undefined,
};

const OnboardingContext = createContext<OnboardingContextValue>(unavailableOnboarding);

function requiredStepsComplete(progress: OnboardingProgress) {
  return progress.openedTrailhead
    && progress.followedConnection
    && progress.savedRepository;
}

export function currentOnboardingStep(
  progress: OnboardingProgress | undefined,
): OnboardingStep | null {
  if (!progress?.openedTrailhead) return 'trailhead';
  if (!progress.followedConnection) return 'connection';
  if (!progress.savedRepository) return 'repository';
  return null;
}

export function OnboardingProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const attemptedBegin = useRef(false);
  const attemptedCompletion = useRef(false);
  const [collapsed, setCollapsed] = useState(false);
  const [completionVisible, setCompletionVisible] = useState(false);
  const [locallyDismissed, setLocallyDismissed] = useState(false);

  const progressQuery = useQuery({
    queryKey: onboardingQueryKey,
    queryFn: () => api.getOnboardingProgress(),
    retry: false,
  });
  const beginMutation = useMutation({
    mutationFn: () => api.beginOnboarding(),
    onSuccess: (progress) => queryClient.setQueryData(onboardingQueryKey, progress),
    retry: false,
  });
  const dismissMutation = useMutation({
    mutationFn: () => api.dismissOnboarding(),
    onMutate: async () => {
      await queryClient.cancelQueries({ queryKey: onboardingQueryKey });
      const previous = queryClient.getQueryData<OnboardingProgress>(onboardingQueryKey);
      if (previous) {
        queryClient.setQueryData(onboardingQueryKey, {
          ...previous,
          status: 'DISMISSED',
          dismissedAt: new Date().toISOString(),
        });
      }
      return { previous };
    },
    onSuccess: (progress) => queryClient.setQueryData(onboardingQueryKey, progress),
    onError: (_error, _variables, context) => {
      setLocallyDismissed(false);
      if (context?.previous) queryClient.setQueryData(onboardingQueryKey, context.previous);
    },
    retry: false,
  });
  const restartMutation = useMutation({
    mutationFn: () => api.restartOnboarding(),
    onSuccess: (progress) => {
      queryClient.setQueryData(onboardingQueryKey, progress);
      setCollapsed(false);
      setCompletionVisible(false);
      setLocallyDismissed(false);
    },
    retry: false,
  });
  const completeMutation = useMutation({
    mutationFn: () => api.completeOnboarding(),
    onSuccess: (progress) => {
      queryClient.setQueryData(onboardingQueryKey, progress);
      setCompletionVisible(true);
    },
    retry: false,
  });

  const progress = progressQuery.data;
  useEffect(() => {
    if (progress?.status !== 'NOT_STARTED' || attemptedBegin.current) return;
    attemptedBegin.current = true;
    beginMutation.mutate();
  }, [beginMutation, progress?.status]);

  useEffect(() => {
    if (progress?.status !== 'IN_PROGRESS' || !requiredStepsComplete(progress)) {
      attemptedCompletion.current = false;
      return;
    }
    if (attemptedCompletion.current) return;
    attemptedCompletion.current = true;
    completeMutation.mutate();
  }, [completeMutation, progress]);

  const active = !locallyDismissed
    && (progress?.status === 'NOT_STARTED' || progress?.status === 'IN_PROGRESS');
  const errorValue = progressQuery.error
    ?? beginMutation.error
    ?? dismissMutation.error
    ?? restartMutation.error
    ?? completeMutation.error;
  const value = useMemo<OnboardingContextValue>(() => ({
    progress,
    loading: progressQuery.isPending || beginMutation.isPending,
    error: errorValue instanceof Error ? errorValue : null,
    active,
    collapsed,
    completionVisible,
    currentStep: currentOnboardingStep(progress),
    dismissPending: dismissMutation.isPending,
    restartPending: restartMutation.isPending,
    setCollapsed,
    dismiss: () => {
      setLocallyDismissed(true);
      dismissMutation.mutate();
    },
    restart: () => restartMutation.mutateAsync(),
    retry: async () => {
      attemptedBegin.current = false;
      return progressQuery.refetch();
    },
    refreshProgress: () => queryClient.invalidateQueries({ queryKey: onboardingQueryKey }),
    hideCompletion: () => setCompletionVisible(false),
  }), [
    active,
    beginMutation.isPending,
    collapsed,
    completionVisible,
    dismissMutation,
    errorValue,
    progress,
    progressQuery,
    queryClient,
    restartMutation,
    locallyDismissed,
  ]);

  return <OnboardingContext.Provider value={value}>{children}</OnboardingContext.Provider>;
}

export function useOnboarding() {
  return useContext(OnboardingContext);
}
