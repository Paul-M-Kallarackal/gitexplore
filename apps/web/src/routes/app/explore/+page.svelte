<script lang="ts">
	import { browser } from '$app/environment';
	import { goto } from '$app/navigation';
	import { createQuery } from '@tanstack/svelte-query';
	import type { Bookmark } from '@gitexplore/api-client';
	import type { PageProps } from './$types';
	import {
		ArrowRight,
		Bookmark as BookmarkIcon,
		GitBranch,
		Search,
		Sparkles,
		Users
	} from 'lucide-svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { createBrowserApi } from '$lib/api';
	import {
		buildExploreHref,
		isLikelyGitHubLogin,
		normalizeLoginInput
	} from '$lib/graph-navigation';

	let { data }: PageProps = $props();

	let search = $state('');
	let searchInitialized = $state(false);
	let validationError = $state<string | null>(null);

	const accountLogin = $derived(data.authStatus.account?.login ?? '');

	$effect(() => {
		if (!searchInitialized) {
			search = data.authStatus.account?.login ?? '';
			searchInitialized = true;
		}
	});

	const bookmarksQuery = createQuery(() => ({
		queryKey: ['bookmarks'],
		queryFn: () => createBrowserApi(data.apiBaseUrl).getBookmarks(),
		enabled: browser,
		staleTime: 60_000,
		retry: false
	}));

	const savedPeople = $derived(
		(bookmarksQuery.data ?? [])
			.filter(
				(bookmark: Bookmark): bookmark is Bookmark & {
					target: { GitHubUser: { login: string } };
				} => 'GitHubUser' in bookmark.target
			)
			.slice(0, 8)
	);

	function openGraph(rawLogin: string) {
		const login = normalizeLoginInput(rawLogin);
		if (!isLikelyGitHubLogin(login)) {
			validationError = 'Enter a GitHub username, @handle, or github.com profile URL.';
			return;
		}

		validationError = null;
		void goto(buildExploreHref(login));
	}

	function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		openGraph(search);
	}
</script>

<svelte:head>
	<title>Explore · GitExplore</title>
</svelte:head>

<PageHeader
	eyebrow="Graph explorer"
	title="Follow the people. Find the work everyone else missed."
	description="Start with any GitHub account, move through follower and following lanes, and save the strongest repositories without losing your path."
/>

<div class="space-y-[var(--space-6)]">
	<section
		aria-labelledby="explore-start-title"
		class="relative overflow-hidden rounded-[var(--radius-2xl)] border border-[var(--border)] bg-[var(--card)] p-[var(--space-6)] shadow-[var(--shadow-card)] md:p-[var(--space-10)]"
	>
		<div
			aria-hidden="true"
			class="pointer-events-none absolute -right-20 -top-24 size-72 rounded-[var(--radius-pill)] bg-[color-mix(in_srgb,var(--primary)_12%,transparent)] blur-3xl"
		></div>
		<div class="relative grid gap-[var(--space-8)] lg:grid-cols-[minmax(0,1.25fr)_minmax(18rem,0.75fr)] lg:items-center">
			<div>
				<p class="eyebrow">Choose your first node</p>
				<h2
					id="explore-start-title"
					class="mt-[var(--space-3)] max-w-2xl text-3xl font-semibold leading-[var(--type-leading-heading)] tracking-[var(--type-tracking-tight)] md:text-4xl"
				>
					Whose corner of GitHub do you want to understand?
				</h2>
				<p class="mt-[var(--space-4)] max-w-xl text-sm leading-6 text-[var(--muted-foreground)]">
					Each click becomes a durable URL step. Go deep, double back, or share the exact trail that led to a repository.
				</p>

				<form class="mt-[var(--space-6)] max-w-2xl" onsubmit={handleSubmit}>
					<label for="graph-login" class="text-sm font-semibold">GitHub username</label>
					<div class="mt-[var(--space-2)] flex flex-col gap-[var(--space-2)] sm:flex-row">
						<div class="relative min-w-0 flex-1">
							<Search
								aria-hidden="true"
								size={18}
								class="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-[var(--muted-foreground)]"
							/>
							<input
								id="graph-login"
								name="github-login"
								bind:value={search}
								required
								aria-describedby={validationError ? 'graph-login-error' : 'graph-login-hint'}
								aria-invalid={validationError ? 'true' : undefined}
								autocomplete="off"
								autocapitalize="none"
								spellcheck={false}
								placeholder="octocat or github.com/octocat"
								class="min-h-12 w-full rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--input)_65%,var(--foreground))] bg-[var(--surface)] pl-11 pr-4 text-base text-[var(--foreground)] transition-colors duration-[var(--motion-duration-fast)] placeholder:text-[var(--muted-foreground)] hover:border-[var(--ring)]"
							/>
						</div>
						<button
							type="submit"
							class="inline-flex min-h-12 items-center justify-center gap-[var(--space-2)] rounded-[var(--radius-md)] bg-[var(--primary)] px-[var(--space-5)] text-sm font-semibold text-[var(--primary-foreground)] transition-opacity duration-[var(--motion-duration-fast)] hover:opacity-90"
						>
							Explore graph
							<ArrowRight aria-hidden="true" size={17} />
						</button>
					</div>
					{#if validationError}
						<p id="graph-login-error" class="mt-[var(--space-2)] text-sm text-[var(--destructive)]" role="alert">
							{validationError}
						</p>
					{:else}
						<p id="graph-login-hint" class="mt-[var(--space-2)] text-xs text-[var(--muted-foreground)]">
							Usernames are case-insensitive. Profile URLs work too.
						</p>
					{/if}
				</form>
			</div>

			<div class="rounded-[var(--radius-xl)] border border-[var(--border)] bg-[var(--surface-inset)] p-[var(--space-5)]">
				<div class="flex items-center gap-[var(--space-3)]">
					<span
						class="grid size-11 place-items-center rounded-[var(--radius-pill)] bg-[var(--accent)] text-[var(--accent-foreground)]"
					>
						<GitBranch aria-hidden="true" size={20} />
					</span>
					<div>
						<p class="text-xs font-semibold uppercase tracking-[var(--type-tracking-caps)] text-[var(--muted-foreground)]">
							Connected account
						</p>
						<p class="mt-0.5 font-semibold">@{accountLogin}</p>
					</div>
				</div>

				{#if accountLogin}
					<a
						href={buildExploreHref(accountLogin)}
						class="mt-[var(--space-5)] inline-flex min-h-[var(--control-hit-target)] w-full items-center justify-between gap-[var(--space-3)] rounded-[var(--radius-md)] border border-[var(--border-strong)] bg-[var(--surface)] px-[var(--space-4)] text-sm font-semibold transition-colors duration-[var(--motion-duration-fast)] hover:bg-[var(--muted)]"
					>
						Explore my network
						<ArrowRight aria-hidden="true" size={16} />
					</a>
				{/if}

				<div class="mt-[var(--space-5)] grid grid-cols-3 gap-[var(--space-2)] text-center">
					<div class="rounded-[var(--radius-md)] bg-[var(--surface)] px-[var(--space-2)] py-[var(--space-3)]">
						<Users aria-hidden="true" size={16} class="mx-auto text-[var(--primary)]" />
						<p class="mt-[var(--space-2)] text-xs text-[var(--muted-foreground)]">People</p>
					</div>
					<div class="rounded-[var(--radius-md)] bg-[var(--surface)] px-[var(--space-2)] py-[var(--space-3)]">
						<Sparkles aria-hidden="true" size={16} class="mx-auto text-[var(--primary)]" />
						<p class="mt-[var(--space-2)] text-xs text-[var(--muted-foreground)]">Signals</p>
					</div>
					<div class="rounded-[var(--radius-md)] bg-[var(--surface)] px-[var(--space-2)] py-[var(--space-3)]">
						<BookmarkIcon aria-hidden="true" size={16} class="mx-auto text-[var(--primary)]" />
						<p class="mt-[var(--space-2)] text-xs text-[var(--muted-foreground)]">Saves</p>
					</div>
				</div>
			</div>
		</div>
	</section>

	<section aria-labelledby="saved-people-title">
		<div class="flex flex-wrap items-end justify-between gap-[var(--space-4)]">
			<div>
				<p class="eyebrow">Saved people</p>
				<h2 id="saved-people-title" class="mt-[var(--space-2)] text-2xl font-semibold tracking-[var(--type-tracking-tight)]">
					Restart from a trusted signal
				</h2>
			</div>
			<a
				href="/app/bookmarks"
				class="inline-flex min-h-[var(--control-hit-target)] items-center rounded-[var(--radius-md)] px-[var(--space-3)] text-sm font-semibold text-[var(--primary)] transition-colors duration-[var(--motion-duration-fast)] hover:bg-[var(--muted)]"
			>
				Manage bookmarks
			</a>
		</div>

		{#if bookmarksQuery.isPending}
			<div class="mt-[var(--space-4)] grid gap-[var(--space-3)] sm:grid-cols-2 lg:grid-cols-4" aria-busy="true" aria-live="polite">
				<span class="sr-only">Loading saved people.</span>
				{#each Array(4) as _}
					<div aria-hidden="true" class="h-20 animate-pulse rounded-[var(--radius-lg)] bg-[var(--muted)] motion-reduce:animate-none"></div>
				{/each}
			</div>
		{:else if bookmarksQuery.isError}
			<div class="mt-[var(--space-4)] rounded-[var(--radius-xl)] border border-[var(--destructive)] bg-[var(--destructive-muted)] p-[var(--space-5)] text-sm text-[var(--destructive-muted-foreground)]" role="alert">
				Saved people could not be loaded. You can still start from a GitHub username above.
			</div>
		{:else if savedPeople.length}
			<ul class="mt-[var(--space-4)] grid gap-[var(--space-3)] sm:grid-cols-2 lg:grid-cols-4" role="list">
				{#each savedPeople as bookmark (bookmark.id)}
					<li>
						<a
							href={buildExploreHref(bookmark.target.GitHubUser.login)}
							class="group flex min-h-20 items-center justify-between gap-[var(--space-3)] rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--card)] px-[var(--space-4)] shadow-[var(--shadow-soft)] transition-[border-color,transform] duration-[var(--motion-duration-fast)] hover:-translate-y-0.5 hover:border-[var(--border-strong)] motion-reduce:transform-none"
						>
							<span class="min-w-0">
								<span class="block truncate font-semibold">@{bookmark.target.GitHubUser.login}</span>
								<span class="mt-1 block truncate text-xs text-[var(--muted-foreground)]">
									{bookmark.note || 'Saved graph entry'}
								</span>
							</span>
							<ArrowRight
								aria-hidden="true"
								size={16}
								class="shrink-0 text-[var(--muted-foreground)] transition-transform duration-[var(--motion-duration-fast)] group-hover:translate-x-0.5 motion-reduce:transform-none"
							/>
						</a>
					</li>
				{/each}
			</ul>
		{:else}
			<div class="mt-[var(--space-4)] rounded-[var(--radius-xl)] border border-dashed border-[var(--border-strong)] bg-[var(--surface)] p-[var(--space-6)]">
				<p class="font-semibold">No people bookmarked yet.</p>
				<p class="mt-[var(--space-1)] text-sm text-[var(--muted-foreground)]">
					Start with your own network, then save people who consistently lead you somewhere interesting.
				</p>
			</div>
		{/if}
	</section>
</div>
