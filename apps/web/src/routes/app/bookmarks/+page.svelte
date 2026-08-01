<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import type { BookmarkTarget } from '@gitexplore/api-client';
	import type { PageProps } from './$types';
	import { createBrowserApi } from '$lib/api';
	import { bookmarkKind, describeBookmarkTarget, formatTimestamp } from '$lib/format';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { BookmarkListItem, Button, Card, DetailPanel, EmptyState, ErrorState, Input } from '@gitexplore/ui';

	let { data }: PageProps = $props();

	const api = $derived(createBrowserApi(data.apiBaseUrl));
	const queryClient = useQueryClient();

	let search = $state('');
	let targetType = $state<'user' | 'repository'>('user');
	let targetValue = $state('');
	let note = $state('');
	let selectedCategories = $state<string[]>([]);
	let selectedBookmarkId = $state<string | null>(null);

	const bookmarksQuery = createQuery(() => ({
		queryKey: ['bookmarks'],
		queryFn: () => api.getBookmarks()
	}));

	const categoriesQuery = createQuery(() => ({
		queryKey: ['categories'],
		queryFn: () => api.getCategories()
	}));

	const addBookmarkMutation = createMutation(() => ({
		mutationFn: (payload: { target: BookmarkTarget; categories: string[]; note?: string | null }) =>
			api.addBookmark(payload),
		onSuccess: async () => {
			targetValue = '';
			note = '';
			selectedCategories = [];
			await queryClient.invalidateQueries({ queryKey: ['bookmarks'] });
		}
	}));

	const filteredBookmarks = $derived.by(() => {
		const normalized = search.trim().toLowerCase();
		const bookmarks = bookmarksQuery.data ?? [];
		if (!normalized) {
			return bookmarks;
		}

		return bookmarks.filter((bookmark) => {
			const target = describeBookmarkTarget(bookmark.target).toLowerCase();
			const bookmarkNote = (bookmark.note ?? '').toLowerCase();
			return (
				target.includes(normalized) ||
				bookmarkNote.includes(normalized) ||
				bookmark.categories.some((category) => category.toLowerCase().includes(normalized))
			);
		});
	});

	const selectedBookmark = $derived(
		filteredBookmarks.find((bookmark) => bookmark.id === selectedBookmarkId) ?? filteredBookmarks[0] ?? null
	);

	$effect(() => {
		if (!selectedBookmarkId && filteredBookmarks[0]) {
			selectedBookmarkId = filteredBookmarks[0].id;
		}
	});

	function toggleCategory(name: string) {
		selectedCategories = selectedCategories.includes(name)
			? selectedCategories.filter((value) => value !== name)
			: [...selectedCategories, name];
	}

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (!targetValue.trim()) {
			return;
		}

		const target =
			targetType === 'user'
				? { GitHubUser: { login: targetValue.trim() } }
				: { GitHubRepository: { full_name: targetValue.trim() } };

		await addBookmarkMutation.mutateAsync({
			target,
			categories: selectedCategories,
			note: note.trim() || null
		});
	}
</script>

<svelte:head>
	<title>Saved nodes · GitExplore</title>
</svelte:head>

<PageHeader
	eyebrow="Bookmarks"
	title="Save people and repositories as durable restart points"
	description="Bookmarks are the user-curated layer over imported GitHub facts. Search the list, tag entries with categories, and keep one detail pane anchored while you move."
/>

<div class="data-grid">
	<div class="space-y-4">
		<Card class="panel p-5">
			<div class="grid gap-4 lg:grid-cols-[1fr_1fr]">
				<div>
					<p class="eyebrow">Add bookmark</p>
					<h2 class="mt-3 text-2xl">New saved node</h2>
					<form class="mt-5 space-y-3" onsubmit={handleSubmit}>
						<div class="grid gap-3 sm:grid-cols-2">
							<label class="space-y-2 text-sm">
								<span class="text-[var(--muted-foreground)]">Target type</span>
								<select
									class="min-h-[var(--control-hit-target)] w-full rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--input)_65%,var(--foreground))] bg-[var(--surface)] px-3 text-sm text-[var(--foreground)]"
									name="bookmark-target-type"
									bind:value={targetType}
								>
									<option value="user">GitHub user</option>
									<option value="repository">GitHub repository</option>
								</select>
							</label>
							<label class="space-y-2 text-sm">
								<span class="text-[var(--muted-foreground)]">Identifier</span>
								<Input
									bind:value={targetValue}
									name="bookmark-target"
									required
									placeholder={targetType === 'user' ? 'torvalds' : 'owner/repository'}
								/>
							</label>
						</div>

						<label class="space-y-2 text-sm">
							<span class="text-[var(--muted-foreground)]">Note</span>
						<textarea
								class="min-h-24 w-full rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--input)_65%,var(--foreground))] bg-[var(--surface)] px-3 py-3 text-sm text-[var(--foreground)] outline-none transition-colors duration-[var(--motion-duration-fast)] focus:border-[var(--ring)]"
								name="bookmark-note"
								bind:value={note}
								placeholder="Why this node matters"
							></textarea>
						</label>

						<fieldset class="space-y-2">
							<legend class="text-sm text-[var(--muted-foreground)]">Categories</legend>
							<div class="flex flex-wrap gap-2">
								{#each (categoriesQuery.data ?? []) as category}
									<label class="inline-flex min-h-[var(--control-hit-target)] cursor-pointer items-center gap-2 rounded-[var(--radius-pill)] border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-xs">
										<input
											type="checkbox"
											name="bookmark-category"
											checked={selectedCategories.includes(category.name)}
											onchange={() => toggleCategory(category.name)}
										/>
										<span>{category.name}</span>
									</label>
								{/each}
							</div>
						</fieldset>

						<div class="flex items-center justify-between gap-3">
							<p class="text-xs text-[var(--muted-foreground)]">
								Bookmarks stay portable at the application layer even if the storage backend changes later.
							</p>
							<Button type="submit" disabled={addBookmarkMutation.isPending}>
								{addBookmarkMutation.isPending ? 'Saving…' : 'Add bookmark'}
							</Button>
						</div>
					</form>
					{#if addBookmarkMutation.isSuccess}
						<p class="sr-only" role="status">Bookmark saved.</p>
					{/if}
				</div>

				<div class="rounded-[var(--radius-xl)] border border-[var(--border)] bg-[var(--muted)] p-5">
					<p class="eyebrow">Browse</p>
					<h2 class="mt-3 text-2xl">Filter saved entries</h2>
					<div class="mt-5">
						<label for="bookmark-search" class="sr-only">Search saved entries</label>
						<Input id="bookmark-search" type="search" name="bookmark-search" bind:value={search} placeholder="Search login, repo, note, or category" />
					</div>
					<p class="mt-4 text-sm text-[var(--muted-foreground)]" role="status" aria-live="polite" aria-atomic="true">
						{filteredBookmarks.length} visible bookmark{filteredBookmarks.length === 1 ? '' : 's'}.
					</p>
				</div>
			</div>
		</Card>

		{#if addBookmarkMutation.isError}
			<ErrorState
				title="Bookmark request failed"
				body={addBookmarkMutation.error instanceof Error ? addBookmarkMutation.error.message : 'The bookmark could not be saved.'}
			/>
		{/if}

		<Card class="panel p-5">
			<div class="space-y-3">
				{#if bookmarksQuery.isPending}
					<div class="space-y-3" aria-busy="true" aria-live="polite">
						<span class="sr-only">Loading saved entries.</span>
						<div aria-hidden="true" class="h-24 animate-pulse rounded-2xl bg-[var(--muted)] motion-reduce:animate-none"></div>
						<div aria-hidden="true" class="h-24 animate-pulse rounded-2xl bg-[var(--muted)] motion-reduce:animate-none"></div>
					</div>
				{:else if bookmarksQuery.isError}
					<ErrorState
						title="Saved entries unavailable"
						body={bookmarksQuery.error instanceof Error ? bookmarksQuery.error.message : 'The saved entries could not be loaded.'}
					/>
				{:else if filteredBookmarks.length}
					<ul class="space-y-3" role="list">
						{#each filteredBookmarks as bookmark (bookmark.id)}
							<li>
								<button
									class="block w-full text-left"
									aria-pressed={bookmark.id === selectedBookmark?.id}
									onclick={() => (selectedBookmarkId = bookmark.id)}
								>
									<BookmarkListItem bookmark={bookmark} selected={bookmark.id === selectedBookmark?.id} />
								</button>
							</li>
						{/each}
					</ul>
				{:else}
					<EmptyState
						title="No bookmarks match"
						body="If the sync already ran, add a person or repository above and it will appear here immediately."
					/>
				{/if}
			</div>
		</Card>
	</div>

	{#if selectedBookmark}
		<DetailPanel
			title={describeBookmarkTarget(selectedBookmark.target)}
			subtitle={selectedBookmark.note ?? 'No note attached to this bookmark.'}
			metadata={[
				{ label: 'Kind', value: bookmarkKind(selectedBookmark.target) },
				{
					label: 'Categories',
					value: selectedBookmark.categories.length ? selectedBookmark.categories.join(', ') : 'Uncategorized'
				},
				{ label: 'Saved at', value: formatTimestamp(selectedBookmark.created_at) }
			]}
		/>
	{:else}
		<div class="sticky top-24 rounded-[var(--radius-xl)] border border-dashed border-[var(--border-strong)] bg-[color-mix(in_srgb,var(--surface)_84%,transparent)] p-6 text-sm text-[var(--muted-foreground)]">
			Select a bookmark to inspect it in place.
		</div>
	{/if}
</div>
