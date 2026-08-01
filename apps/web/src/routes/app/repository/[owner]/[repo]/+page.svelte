<script lang="ts">
	import { browser } from '$app/environment';
	import { page } from '$app/state';
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import type { PageProps } from './$types';
	import {
		AlertCircle,
		ArrowLeft,
		ArrowRight,
		Bookmark,
		Check,
		ExternalLink,
		GitBranch,
		RefreshCcw,
		Sparkles
	} from 'lucide-svelte';
	import ContributorStrip from '$lib/components/ContributorStrip.svelte';
	import { createBrowserApi } from '$lib/api';

	let { data }: PageProps = $props();
	let previewFailed = $state(false);

	const owner = $derived(page.params.owner?.trim() ?? '');
	const repo = $derived(page.params.repo?.trim() ?? '');
	const fullName = $derived(`${owner}/${repo}`);
	const validRepository = $derived(
		/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/.test(owner) && /^[A-Za-z0-9._-]{1,100}$/.test(repo)
	);
	const api = $derived(createBrowserApi(data.apiBaseUrl));
	const queryClient = useQueryClient();

	const insightsQuery = createQuery(() => ({
		queryKey: ['repository-insights', fullName.toLowerCase(), 16],
		queryFn: () => api.getRepositoryInsights(fullName, 16),
		enabled: browser && validRepository,
		staleTime: 10 * 60_000,
		gcTime: 60 * 60_000,
		retry: false
	}));

	const bookmarksQuery = createQuery(() => ({
		queryKey: ['bookmarks'],
		queryFn: () => api.getBookmarks(),
		enabled: browser,
		staleTime: 60_000,
		retry: false
	}));

	const saveMutation = createMutation(() => ({
		mutationFn: () => api.saveRepository(fullName, [], null),
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['bookmarks'] });
		},
		retry: false
	}));

	const saved = $derived(
		(bookmarksQuery.data ?? []).some(
			(bookmark) =>
				'GitHubRepository' in bookmark.target &&
				bookmark.target.GitHubRepository.full_name.toLowerCase() === fullName.toLowerCase()
		)
	);

	function cacheLabel(value: string) {
		return value.toLowerCase().replaceAll('_', ' ');
	}
</script>

<svelte:head>
	<title>{validRepository ? `${fullName} · GitExplore` : 'Repository · GitExplore'}</title>
</svelte:head>

<div class="repository-page">
	<a href="/app/explore" class="back-link"><ArrowLeft aria-hidden="true" size={16} /> Back to discovery</a>

	{#if !validRepository}
		<section class="state-card error" role="alert">
			<AlertCircle aria-hidden="true" size={22} />
			<div><h1>That repository name is not valid.</h1><p>Open a repository from the discovery feed or enter owner/name.</p></div>
		</section>
	{:else}
		<section class="repo-hero" aria-labelledby="repo-title">
			<div class="preview" class:preview-fallback={previewFailed}>
				{#if !previewFailed}
					<img
						src={`https://opengraph.githubassets.com/gitexplore-atlas/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}`}
						alt={`GitHub social preview for ${fullName}`}
						width="1280"
						height="640"
						decoding="async"
						onerror={() => (previewFailed = true)}
					/>
				{:else}
					<GitBranch aria-hidden="true" size={42} />
				{/if}
				<div class="preview-shade"></div>
			</div>

			<div class="repo-heading">
				<div class="repo-owner">
					<img
						src={`https://github.com/${encodeURIComponent(owner)}.png?size=112`}
						alt=""
						width="56"
						height="56"
						decoding="async"
					/>
					<div>
						<p class="eyebrow">Repository node</p>
						<p class="owner-name">{owner}</p>
					</div>
				</div>
				<h1 id="repo-title">{repo}</h1>
				<p>Inspect the people carrying the work, then continue through their public graphs.</p>
			</div>

			<div class="repo-actions">
				<a href={`https://github.com/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}`} target="_blank" rel="noreferrer">
					<GitBranch aria-hidden="true" size={16} /> Open on GitHub <ExternalLink aria-hidden="true" size={14} /><span class="sr-only"> (opens in a new tab)</span>
				</a>
				<button type="button" disabled={saved || saveMutation.isPending} onclick={() => saveMutation.mutate()} class:saved>
					{#if saved}<Check aria-hidden="true" size={16} /> Saved{:else}<Bookmark aria-hidden="true" size={16} /> {saveMutation.isPending ? 'Saving…' : 'Save repository'}{/if}
				</button>
			</div>
		</section>

		{#if saveMutation.isError}
			<div class="notice error" role="alert"><AlertCircle aria-hidden="true" size={17} /><span>{saveMutation.error instanceof Error ? saveMutation.error.message : 'The repository could not be saved.'}</span></div>
		{/if}
		{#if saveMutation.isSuccess}
			<p class="sr-only" role="status">{fullName} saved.</p>
		{/if}

		<div class="insight-grid">
			<div class="insight-main">
				{#if insightsQuery.isPending}
					<section class="loading" aria-busy="true" aria-live="polite">
						<div class="orbit" aria-hidden="true"><span></span><Sparkles size={20} /></div>
						<div><h2>Reading contributor signal</h2><p>The cached result appears first; a stale entry refreshes in the background.</p></div>
					</section>
				{:else if insightsQuery.data}
					<ContributorStrip insight={insightsQuery.data} />
				{:else if insightsQuery.isError}
					<section class="state-card error" role="alert">
						<AlertCircle aria-hidden="true" size={22} />
						<div><h2>Contributor activity is unavailable.</h2><p>{insightsQuery.error instanceof Error ? insightsQuery.error.message : 'The request failed.'}</p></div>
						<button type="button" onclick={() => insightsQuery.refetch()}><RefreshCcw aria-hidden="true" size={15} /> Try again</button>
					</section>
				{/if}
			</div>

			<aside class="field-notes">
				<p class="eyebrow">Field notes</p>
				<h2>How to read this node</h2>
				<div class="note-list">
					<div><strong>Contributor order</strong><p>GitHub-attributed commit totals across the repository’s public history.</p></div>
					<div><strong>Cache policy</strong><p>Fresh for 24 hours. Stale data remains visible while one shared refresh runs.</p></div>
					{#if insightsQuery.data}
						<div><strong>Current state</strong><p>{cacheLabel(insightsQuery.data.cacheStatus)} · {insightsQuery.data.sourceComplete ? 'complete source result' : 'partial source result'}</p></div>
					{/if}
				</div>
				<a href={`/app/explore/${encodeURIComponent(owner)}`} class="continue-link">
					Explore @{owner} <ArrowRight aria-hidden="true" size={16} />
				</a>
			</aside>
		</div>
	{/if}
</div>

<style>
	.repository-page { display: grid; gap: var(--space-4); }
	.back-link { display: inline-flex; width: fit-content; min-height: var(--control-hit-target); align-items: center; gap: var(--space-2); border-radius: var(--radius-md); padding-inline: var(--space-2); color: var(--muted-foreground); font-size: var(--type-size-sm); font-weight: 650; text-decoration: none; }
	.back-link:hover { background: var(--muted); color: var(--foreground); }
	.repo-hero { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(18rem, 0.8fr); grid-template-rows: minmax(14rem, 22rem) auto; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-xl); background: var(--card); box-shadow: var(--shadow-card); }
	.preview { position: relative; grid-row: 1 / 3; min-height: 22rem; overflow: hidden; background: var(--accent); color: var(--accent-foreground); }
	.preview img { width: 100%; height: 100%; object-fit: cover; }
	.preview-fallback { display: grid; place-items: center; background-image: radial-gradient(circle at 30% 30%, color-mix(in srgb, var(--primary) 24%, transparent), transparent 42%), linear-gradient(145deg, var(--accent), var(--surface-inset)); }
	.preview-shade { position: absolute; inset: 0; box-shadow: inset -32px 0 80px rgb(36 27 43 / 0.12); pointer-events: none; }
	.repo-heading { align-self: end; padding: var(--space-6); }
	.repo-owner { display: flex; align-items: center; gap: var(--space-3); }
	.repo-owner img { width: 3.5rem; height: 3.5rem; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--surface-inset); object-fit: cover; }
	.owner-name { margin: var(--space-1) 0 0; color: var(--muted-foreground); font-family: var(--font-mono); font-size: var(--type-size-sm); }
	.repo-heading h1 { overflow-wrap: anywhere; margin: var(--space-5) 0 0; font-size: clamp(2.5rem, 6vw, 4.75rem); font-weight: 680; letter-spacing: -0.06em; line-height: 0.92; }
	.repo-heading > p { max-width: 34rem; margin: var(--space-4) 0 0; color: var(--muted-foreground); font-size: var(--type-size-sm); line-height: 1.6; }
	.repo-actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); padding: 0 var(--space-6) var(--space-6); }
	.repo-actions a, .repo-actions button, .state-card button { display: inline-flex; min-height: var(--control-hit-target); align-items: center; justify-content: center; gap: var(--space-2); border: 1px solid var(--border-strong); border-radius: var(--radius-md); background: var(--surface); padding-inline: var(--space-4); color: var(--foreground); font: inherit; font-size: var(--type-size-sm); font-weight: 700; text-decoration: none; cursor: pointer; }
	.repo-actions button { border-color: var(--primary); background: var(--primary); color: var(--primary-foreground); }
	.repo-actions button.saved { border-color: var(--success); background: var(--success-muted); color: var(--success-muted-foreground); }
	.repo-actions button:disabled { cursor: default; opacity: var(--effect-disabled-opacity); }
	.repo-actions button.saved:disabled { opacity: 1; }
	.insight-grid { display: grid; gap: var(--space-4); align-items: start; }
	.field-notes { border: 1px solid var(--border); border-radius: var(--radius-xl); background: var(--card); padding: var(--space-5); box-shadow: var(--shadow-soft); }
	.field-notes h2 { margin: var(--space-2) 0 0; font-size: var(--type-size-xl); font-weight: 670; letter-spacing: var(--type-tracking-heading); }
	.note-list { display: grid; gap: var(--space-4); margin-top: var(--space-5); }
	.note-list div { padding-bottom: var(--space-4); border-bottom: 1px solid var(--border); }
	.note-list strong { font-size: var(--type-size-sm); }
	.note-list p { margin: var(--space-1) 0 0; color: var(--muted-foreground); font-size: var(--type-size-xs); line-height: 1.55; }
	.continue-link { display: flex; min-height: var(--control-hit-target); align-items: center; justify-content: space-between; gap: var(--space-2); margin-top: var(--space-5); border-radius: var(--radius-md); background: var(--accent); padding-inline: var(--space-4); color: var(--accent-foreground); font-size: var(--type-size-sm); font-weight: 700; text-decoration: none; }
	.loading, .state-card { display: flex; min-height: 18rem; align-items: center; justify-content: center; gap: var(--space-4); border: 1px solid var(--border); border-radius: var(--radius-xl); background: var(--card); padding: var(--space-8); box-shadow: var(--shadow-soft); }
	.loading h2, .state-card h1, .state-card h2 { margin: 0; font-size: var(--type-size-xl); }
	.loading p, .state-card p { margin: var(--space-2) 0 0; color: var(--muted-foreground); font-size: var(--type-size-sm); }
	.state-card { flex-wrap: wrap; }
	.state-card.error > :global(svg) { color: var(--destructive); }
	.orbit { position: relative; display: grid; width: 3.5rem; height: 3.5rem; flex: 0 0 auto; place-items: center; border: 1px dashed var(--primary); border-radius: var(--radius-pill); color: var(--primary); }
	.orbit span { position: absolute; width: 0.5rem; height: 0.5rem; border-radius: var(--radius-pill); background: var(--success); animation: orbit 1.8s linear infinite; }
	.notice { display: flex; min-height: var(--control-hit-target); align-items: center; gap: var(--space-3); border: 1px solid var(--destructive); border-radius: var(--radius-lg); background: var(--destructive-muted); padding: var(--space-3) var(--space-4); color: var(--destructive-muted-foreground); font-size: var(--type-size-sm); }
	@keyframes orbit { from { transform: rotate(0deg) translateX(1.75rem) rotate(0deg); } to { transform: rotate(360deg) translateX(1.75rem) rotate(-360deg); } }

	@media (min-width: 64rem) {
		.insight-grid { grid-template-columns: minmax(0, 1fr) minmax(18rem, 22rem); }
		.field-notes { position: sticky; top: 5rem; }
	}

	@media (max-width: 52rem) {
		.repo-hero { grid-template-columns: 1fr; grid-template-rows: minmax(13rem, 42vw) auto auto; }
		.preview { grid-row: auto; min-height: 13rem; }
	}

	@media (max-width: 36rem) {
		.repo-heading, .repo-actions { padding-inline: var(--space-4); }
		.repo-actions { align-items: stretch; flex-direction: column; }
		.repo-actions a, .repo-actions button { width: 100%; }
	}

	@media (prefers-reduced-motion: reduce) {
		.orbit span { animation: none; }
	}
</style>
