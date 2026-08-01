<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import type { PageProps } from './$types';
	import { createBrowserApi } from '$lib/api';
	import { seedLabel, snapshotSummary } from '$lib/format';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { Card, DetailPanel, EmptyState, ErrorState, Input } from '@gitexplore/ui';

	let { data }: PageProps = $props();

	const api = $derived(createBrowserApi(data.apiBaseUrl));

	let search = $state('');
	let selectedSnapshotId = $state<string | null>(null);

	const snapshotsQuery = createQuery(() => ({
		queryKey: ['exploration-snapshots'],
		queryFn: () => api.getExplorationSnapshots()
	}));

	const filteredSnapshots = $derived.by(() => {
		const normalized = search.trim().toLowerCase();
		const snapshots = snapshotsQuery.data ?? [];
		if (!normalized) {
			return snapshots;
		}

		return snapshots.filter((snapshot) => {
			const label = seedLabel(snapshot.seed).toLowerCase();
			return label.includes(normalized) || snapshot.id.toLowerCase().includes(normalized);
		});
	});

	const selectedSnapshot = $derived(
		filteredSnapshots.find((snapshot) => snapshot.id === selectedSnapshotId) ?? filteredSnapshots[0] ?? null
	);

	$effect(() => {
		if (!selectedSnapshotId && filteredSnapshots[0]) {
			selectedSnapshotId = filteredSnapshots[0].id;
		}
	});
</script>

<svelte:head>
	<title>Saved trails · GitExplore</title>
</svelte:head>

<PageHeader
	eyebrow="Snapshots"
	title="Review saved exploration sessions"
	description="Every exploration response saves a restartable snapshot. Keep this list searchable so you can jump back into previous traversal contexts quickly."
/>

<div class="data-grid">
	<div class="space-y-4">
		<Card class="panel p-5">
			<p class="eyebrow">Search snapshots</p>
			<h2 class="mt-3 text-2xl">Saved session archive</h2>
			<div class="mt-5">
				<label for="snapshot-search" class="sr-only">Search saved trails</label>
				<Input id="snapshot-search" type="search" name="snapshot-search" bind:value={search} placeholder="Search seed label or snapshot id" />
			</div>
			<p class="mt-3 text-sm text-[var(--muted-foreground)]" role="status" aria-live="polite" aria-atomic="true">
				{filteredSnapshots.length} visible trail{filteredSnapshots.length === 1 ? '' : 's'}.
			</p>
		</Card>

		<Card class="panel p-5">
			<div class="space-y-3">
				{#if snapshotsQuery.isPending}
					<div class="space-y-3" aria-busy="true" aria-live="polite">
						<span class="sr-only">Loading saved trails.</span>
						<div aria-hidden="true" class="h-20 animate-pulse rounded-2xl bg-[var(--muted)] motion-reduce:animate-none"></div>
						<div aria-hidden="true" class="h-20 animate-pulse rounded-2xl bg-[var(--muted)] motion-reduce:animate-none"></div>
					</div>
				{:else if snapshotsQuery.isError}
					<ErrorState
						title="Saved trails unavailable"
						body={snapshotsQuery.error instanceof Error ? snapshotsQuery.error.message : 'The saved trails could not be loaded.'}
					/>
				{:else if filteredSnapshots.length}
					<ul class="space-y-3" role="list">
						{#each filteredSnapshots as snapshot (snapshot.id)}
							<li>
								<button
									class={`block min-h-[var(--control-hit-target)] w-full rounded-[var(--radius-lg)] border p-4 text-left transition-colors duration-[var(--motion-duration-fast)] ${
										selectedSnapshot?.id === snapshot.id
											? 'border-[var(--primary)] bg-[var(--muted)]'
											: 'border-[var(--border)] bg-[var(--surface)] hover:bg-[var(--muted)]'
									}`}
									aria-pressed={selectedSnapshot?.id === snapshot.id}
									onclick={() => (selectedSnapshotId = snapshot.id)}
								>
									<span class="flex items-start justify-between gap-3">
										<span>
											<span class="block text-lg font-semibold">{seedLabel(snapshot.seed)}</span>
											<span class="mt-1 block text-sm text-[var(--muted-foreground)]">{snapshotSummary(snapshot)}</span>
										</span>
										<span class="text-xs text-[var(--muted-foreground)]">
											{new Date(snapshot.generated_at).toLocaleDateString()}
										</span>
									</span>
								</button>
							</li>
						{/each}
					</ul>
				{:else}
					<EmptyState
						title="No snapshots saved"
						body="Run an exploration first. Each successful query creates a saved snapshot you can revisit here."
					/>
				{/if}
			</div>
		</Card>
	</div>

	{#if selectedSnapshot}
		<DetailPanel
			title={seedLabel(selectedSnapshot.seed)}
			subtitle={snapshotSummary(selectedSnapshot)}
			metadata={[
				{ label: 'Snapshot id', value: selectedSnapshot.id },
				{
					label: 'Generated',
					value: new Date(selectedSnapshot.generated_at).toLocaleString()
				},
				{
					label: 'Discovered people',
					value: selectedSnapshot.discovered_people.join(', ') || 'None'
				}
			]}
		/>
	{:else}
		<div class="sticky top-24 rounded-[var(--radius-xl)] border border-dashed border-[var(--border-strong)] bg-[color-mix(in_srgb,var(--surface)_84%,transparent)] p-6 text-sm text-[var(--muted-foreground)]">
			Select a snapshot to inspect its seed and saved discovery set.
		</div>
	{/if}
</div>
