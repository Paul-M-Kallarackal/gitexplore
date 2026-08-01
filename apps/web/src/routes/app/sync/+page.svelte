<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { RefreshCcw } from 'lucide-svelte';
	import type { PageProps } from './$types';
	import { createBrowserApi } from '$lib/api';
	import { formatTimestamp } from '$lib/format';
	import IconAction from '$lib/components/IconAction.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { Button, Card, ErrorState, SyncStatusCard } from '@gitexplore/ui';

	let { data }: PageProps = $props();

	const api = $derived(createBrowserApi(data.apiBaseUrl));
	const queryClient = useQueryClient();

	const syncStatusQuery = createQuery(() => ({
		queryKey: ['sync-status'],
		queryFn: () => api.getSyncStatus(),
		initialData: data.syncStatus
	}));

	const syncMutation = createMutation(() => ({
		mutationFn: () => api.runSync(),
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['sync-status'] });
		}
	}));
</script>

<svelte:head>
	<title>Sync and cache · GitExplore</title>
</svelte:head>

<PageHeader
	eyebrow="Sync"
	title="Manual ingestion only, with clear state"
	description="v1 keeps synchronization explicit. Trigger a refresh when you want a new snapshot of your GitHub network, then inspect the last outcome and timestamps here."
>
	{#snippet actions()}
		<IconAction
			icon={RefreshCcw}
			label="Refresh sync status"
			onclick={() => queryClient.invalidateQueries({ queryKey: ['sync-status'] })}
		/>
		<Button onclick={() => syncMutation.mutate()} disabled={syncMutation.isPending}>
			{syncMutation.isPending ? 'Syncing…' : 'Run sync'}
		</Button>
	{/snippet}
</PageHeader>

<div class="space-y-4">
	{#if syncMutation.isError}
		<ErrorState
			title="Sync failed"
			body={syncMutation.error instanceof Error ? syncMutation.error.message : 'The sync request failed.'}
		/>
	{/if}
	{#if syncMutation.isSuccess}
		<p class="sr-only" role="status">Sync completed.</p>
	{/if}
	{#if syncStatusQuery.isError}
		<ErrorState
			title="Sync status unavailable"
			body={syncStatusQuery.error instanceof Error ? syncStatusQuery.error.message : 'The latest sync status could not be loaded.'}
		/>
	{/if}

	<div role="status" aria-live="polite" aria-busy={syncMutation.isPending || syncStatusQuery.isFetching}>
		<SyncStatusCard status={syncStatusQuery.data ?? data.syncStatus} summary={syncMutation.data ?? null} />
	</div>

	<div class="grid gap-4 lg:grid-cols-3">
		<Card class="panel p-5 lg:col-span-2">
			<p class="eyebrow">Behavior</p>
			<h2 class="mt-3 text-2xl">What this sync does today</h2>
			<div class="mt-5 grid gap-3 md:grid-cols-2">
				<div class="rounded-2xl bg-[var(--muted)] p-4 text-sm text-[var(--muted-foreground)]">
					Fetches the viewer profile, followers, following, starred repositories, and accessible repositories.
				</div>
				<div class="rounded-2xl bg-[var(--muted)] p-4 text-sm text-[var(--muted-foreground)]">
					Normalizes imported GitHub facts separately from bookmarks and exploration snapshots.
				</div>
				<div class="rounded-2xl bg-[var(--muted)] p-4 text-sm text-[var(--muted-foreground)]">
					Persists into the configured backend, currently Neo4j for graph-rich traversal.
				</div>
				<div class="rounded-2xl bg-[var(--muted)] p-4 text-sm text-[var(--muted-foreground)]">
					Leaves scheduling and webhooks out of scope so you can reason about state manually.
				</div>
			</div>
		</Card>

		<Card class="panel p-5">
			<p class="eyebrow">Timestamps</p>
			<h2 class="mt-3 text-2xl">Last known run</h2>
			<div class="mt-5 space-y-3 text-sm">
				<div class="rounded-2xl bg-[var(--muted)] p-4">
					<div class="text-xs uppercase tracking-[0.24em] text-[var(--muted-foreground)]">Recorded at</div>
					<div class="mt-2">{formatTimestamp((syncStatusQuery.data ?? data.syncStatus).last_synced_at)}</div>
				</div>
				<div class="rounded-2xl bg-[var(--muted)] p-4">
					<div class="text-xs uppercase tracking-[0.24em] text-[var(--muted-foreground)]">Last error</div>
					<div class="mt-2">{(syncStatusQuery.data ?? data.syncStatus).last_error ?? 'No error recorded.'}</div>
				</div>
			</div>
		</Card>
	</div>
</div>
