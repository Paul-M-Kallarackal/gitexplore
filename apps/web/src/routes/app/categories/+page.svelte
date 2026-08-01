<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import type { PageProps } from './$types';
	import { createBrowserApi } from '$lib/api';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { categorySummary } from '$lib/format';
	import { Button, Card, DetailPanel, EmptyState, ErrorState, Input } from '@gitexplore/ui';

	let { data }: PageProps = $props();

	const api = $derived(createBrowserApi(data.apiBaseUrl));
	const queryClient = useQueryClient();

	let name = $state('');
	let description = $state('');
	let selectedCategoryName = $state<string | null>(null);

	const categoriesQuery = createQuery(() => ({
		queryKey: ['categories'],
		queryFn: () => api.getCategories()
	}));

	const createCategoryMutation = createMutation(() => ({
		mutationFn: (payload: { name: string; description?: string | null }) => api.createCategory(payload),
		onSuccess: async () => {
			name = '';
			description = '';
			await queryClient.invalidateQueries({ queryKey: ['categories'] });
		}
	}));

	const selectedCategory = $derived(
		(categoriesQuery.data ?? []).find((category) => category.name === selectedCategoryName) ??
			(categoriesQuery.data ?? [])[0] ??
			null
	);

	$effect(() => {
		if (!selectedCategoryName && categoriesQuery.data?.[0]) {
			selectedCategoryName = categoriesQuery.data[0].name;
		}
	});

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (!name.trim()) {
			return;
		}

		await createCategoryMutation.mutateAsync({
			name: name.trim(),
			description: description.trim() || null
		});
	}
</script>

<svelte:head>
	<title>Collections · GitExplore</title>
</svelte:head>

<PageHeader
	eyebrow="Categories"
	title="Organize the graph with your own grouping language"
	description="Categories stay intentionally lightweight in v1. They exist to shape bookmark clusters and to act as exploration seeds without introducing recommendation machinery."
/>

<div class="data-grid">
	<div class="space-y-4">
		<Card class="panel p-5">
			<div class="grid gap-4 lg:grid-cols-[1.1fr_0.9fr]">
				<form class="space-y-3" onsubmit={handleSubmit}>
					<p class="eyebrow">Create category</p>
					<h2 class="mt-3 text-2xl">New cluster</h2>
					<label class="block space-y-2 text-sm">
						<span class="text-[var(--muted-foreground)]">Name</span>
						<Input bind:value={name} name="category-name" required placeholder="Design leaders" />
					</label>
					<label class="block space-y-2 text-sm">
						<span class="text-[var(--muted-foreground)]">Description</span>
						<textarea
							class="min-h-24 w-full rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--input)_65%,var(--foreground))] bg-[var(--surface)] px-3 py-3 text-sm text-[var(--foreground)] outline-none transition-colors duration-[var(--motion-duration-fast)] focus:border-[var(--ring)]"
							name="category-description"
							bind:value={description}
							placeholder="What this grouping is for"
						></textarea>
					</label>
					<Button type="submit" disabled={createCategoryMutation.isPending}>
						{createCategoryMutation.isPending ? 'Creating…' : 'Create category'}
					</Button>
					{#if createCategoryMutation.isSuccess}
						<p class="sr-only" role="status">Category created.</p>
					{/if}
				</form>

				<div class="rounded-[var(--radius-xl)] border border-[var(--border)] bg-[var(--muted)] p-5">
					<p class="eyebrow">Current inventory</p>
					<h2 class="mt-3 text-2xl">{categorySummary(categoriesQuery.data ?? [])}</h2>
					<p class="mt-3 text-sm leading-6 text-[var(--muted-foreground)]">
						Use categories to keep bookmark collections meaningful, then jump into exploration directly from one group.
					</p>
				</div>
			</div>
		</Card>

		{#if createCategoryMutation.isError}
			<ErrorState
				title="Category request failed"
				body={createCategoryMutation.error instanceof Error ? createCategoryMutation.error.message : 'The category could not be created.'}
			/>
		{/if}

		<Card class="panel p-5">
			<div class="space-y-3">
				{#if categoriesQuery.isPending}
					<div class="space-y-3" aria-busy="true" aria-live="polite">
						<span class="sr-only">Loading categories.</span>
						<div aria-hidden="true" class="h-20 animate-pulse rounded-2xl bg-[var(--muted)] motion-reduce:animate-none"></div>
						<div aria-hidden="true" class="h-20 animate-pulse rounded-2xl bg-[var(--muted)] motion-reduce:animate-none"></div>
					</div>
				{:else if categoriesQuery.isError}
					<ErrorState
						title="Categories unavailable"
						body={categoriesQuery.error instanceof Error ? categoriesQuery.error.message : 'The categories could not be loaded.'}
					/>
				{:else if (categoriesQuery.data ?? []).length}
					<ul class="space-y-3" role="list">
						{#each categoriesQuery.data ?? [] as category (category.name)}
							<li>
								<button
									class={`block min-h-[var(--control-hit-target)] w-full rounded-[var(--radius-lg)] border p-4 text-left transition-colors duration-[var(--motion-duration-fast)] ${
										selectedCategory?.name === category.name
											? 'border-[var(--primary)] bg-[var(--muted)]'
											: 'border-[var(--border)] bg-[var(--surface)] hover:bg-[var(--muted)]'
									}`}
									aria-pressed={selectedCategory?.name === category.name}
									onclick={() => (selectedCategoryName = category.name)}
								>
									<span class="flex items-center justify-between gap-3">
										<span>
											<span class="block text-lg font-semibold">{category.name}</span>
											<span class="mt-1 block text-sm text-[var(--muted-foreground)]">
												{category.description ?? 'No description yet.'}
											</span>
										</span>
										<span class="rounded-full bg-[var(--muted)] px-3 py-1 text-xs text-[var(--muted-foreground)]">
											Category
										</span>
									</span>
								</button>
							</li>
						{/each}
					</ul>
				{:else}
					<EmptyState
						title="No categories yet"
						body="Create one above so bookmarks and exploration have a reusable grouping surface."
					/>
				{/if}
			</div>
		</Card>
	</div>

	{#if selectedCategory}
		<DetailPanel
			title={selectedCategory.name}
			subtitle={selectedCategory.description ?? 'No description supplied.'}
			metadata={[
				{ label: 'Role', value: 'Exploration seed and bookmark grouping' },
				{ label: 'Name', value: selectedCategory.name }
			]}
		/>
	{/if}
</div>
