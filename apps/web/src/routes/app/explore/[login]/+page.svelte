<script lang="ts">
	import { browser } from '$app/environment';
	import { page } from '$app/state';
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import {
		GitExploreApiError,
		type RepositoryCandidate,
		type UserNeighborhood
	} from '@gitexplore/api-client';
	import type { PageProps } from './$types';
	import {
		AlertCircle,
		ArrowLeft,
		Compass,
		ExternalLink,
		GitBranch,
		RefreshCcw,
		Sparkles,
		Users
	} from 'lucide-svelte';
	import GraphTrail from '$lib/components/GraphTrail.svelte';
	import PersonLane from '$lib/components/PersonLane.svelte';
	import RepositoryDiscoveryCard from '$lib/components/RepositoryDiscoveryCard.svelte';
	import UserCommitList from '$lib/components/UserCommitList.svelte';
	import { createBrowserApi } from '$lib/api';
	import { isLikelyGitHubLogin, normalizeLoginInput, normalizeTrail } from '$lib/graph-navigation';

	let { data }: PageProps = $props();

	const neighborhoodLimit = 36;
	const queryClient = useQueryClient();
	const attemptedAutoExpansions = new Set<string>();

	let expansionMode = $state<'automatic' | 'manual'>('manual');
	let loadingPhase = $state<'reading' | 'expanding'>('reading');
	let activeSaveFullName = $state<string | null>(null);
	let saveErrors = $state<Record<string, string>>({});
	let activeLane = $state<'followers' | 'following'>('followers');
	let visibleRepositoryCount = $state(8);

	const login = $derived(normalizeLoginInput(page.params.login ?? ''));
	const validLogin = $derived(isLikelyGitHubLogin(login));
	const trail = $derived(normalizeTrail(page.url.searchParams.get('trail'), login));
	const api = $derived(createBrowserApi(data.apiBaseUrl));

	function neighborhoodKey(value: string) {
		return ['user-neighborhood', value.toLowerCase(), neighborhoodLimit] as const;
	}

	async function loadNeighborhood(requestedLogin: string) {
		try {
			loadingPhase = 'reading';
			return await api.getNeighborhood(requestedLogin, neighborhoodLimit);
		} catch (error) {
			if (!(error instanceof GitExploreApiError) || error.code !== 'NOT_FOUND') throw error;

			const normalizedLogin = requestedLogin.toLowerCase();
			attemptedAutoExpansions.add(normalizedLogin);
			expansionMode = 'automatic';
			loadingPhase = 'expanding';
			try {
				return await api.expandUser(requestedLogin, neighborhoodLimit);
			} finally {
				loadingPhase = 'reading';
			}
		}
	}

	const neighborhoodQuery = createQuery(() => ({
		queryKey: neighborhoodKey(login),
		queryFn: () => loadNeighborhood(login),
		enabled: browser && validLogin,
		staleTime: 60_000,
		gcTime: 30 * 60_000,
		retry: false
	}));

	const userInsightsQuery = createQuery(() => ({
		queryKey: ['user-insights', login.toLowerCase(), 12],
		queryFn: () => api.getUserInsights(login, 12),
		enabled: browser && validLogin && Boolean(neighborhoodQuery.data),
		staleTime: 10 * 60_000,
		gcTime: 60 * 60_000,
		retry: false
	}));

	const expandMutation = createMutation(() => ({
		mutationFn: (requestedLogin: string) => api.expandUser(requestedLogin, neighborhoodLimit),
		retry: false,
		onSuccess: async (neighborhood, requestedLogin) => {
			queryClient.setQueryData(neighborhoodKey(requestedLogin), neighborhood);
			await queryClient.invalidateQueries({ queryKey: ['user-insights', requestedLogin.toLowerCase()] });
			await queryClient.invalidateQueries({ queryKey: ['github-rate-limit'] });
		}
	}));

	const saveMutation = createMutation(() => ({
		mutationFn: (fullName: string) => api.saveRepository(fullName, [], null),
		retry: false,
		onSuccess: (_savedRepository, fullName) => {
			queryClient.setQueriesData<UserNeighborhood>({ queryKey: ['user-neighborhood'] }, (current) => {
				if (!current) return current;
				return {
					...current,
					repositories: current.repositories.map((candidate) =>
						candidate.repository.fullName === fullName ? { ...candidate, saved: true } : candidate
					)
				};
			});
			void queryClient.invalidateQueries({ queryKey: ['bookmarks'] });
		}
	}));

	const neighborhood = $derived(neighborhoodQuery.data ?? null);
	const visibleRepositories = $derived(neighborhood?.repositories.slice(0, visibleRepositoryCount) ?? []);
	const hiddenRepositoryCount = $derived(
		Math.max(0, (neighborhood?.repositories.length ?? 0) - visibleRepositories.length)
	);
	const partialCollections = $derived.by(() => {
		const coverage = neighborhood?.coverage;
		if (!coverage) return [];
		return [
			!coverage.followersComplete && 'followers',
			!coverage.followingComplete && 'following',
			!coverage.starredRepositoriesComplete && 'starred repositories',
			!coverage.repositoriesComplete && 'owned repositories'
		].filter((label): label is string => Boolean(label));
	});
	const expandingCurrent = $derived(
		expandMutation.isPending && expandMutation.variables?.toLowerCase() === login.toLowerCase()
	);
	const expeditionLabel = $derived(
		trail.length >= 7 ? 'Pathfinder route' : trail.length >= 3 ? 'Scout route' : 'Trailhead'
	);

	$effect(() => {
		login;
		activeLane = 'followers';
		visibleRepositoryCount = 8;
	});

	$effect(() => {
		const current = neighborhoodQuery.data;
		const normalizedLogin = login.toLowerCase();
		if (!browser || !current || current.lastFetchedAt !== null || attemptedAutoExpansions.has(normalizedLogin)) return;

		attemptedAutoExpansions.add(normalizedLogin);
		expansionMode = 'automatic';
		expandMutation.mutate(login);
	});

	function refreshNeighborhood() {
		expansionMode = 'manual';
		expandMutation.mutate(login);
	}

	function selectLane(lane: 'followers' | 'following', moveFocus = false) {
		activeLane = lane;
		if (moveFocus) {
			document.getElementById(`${lane}-tab`)?.focus();
		}
	}

	function handleLaneKeydown(event: KeyboardEvent) {
		let nextLane: 'followers' | 'following' | null = null;
		if (event.key === 'Home') {
			nextLane = 'followers';
		} else if (event.key === 'End') {
			nextLane = 'following';
		} else if (
			event.key === 'ArrowLeft' ||
			event.key === 'ArrowUp' ||
			event.key === 'ArrowRight' ||
			event.key === 'ArrowDown'
		) {
			nextLane = activeLane === 'followers' ? 'following' : 'followers';
		}

		if (!nextLane) return;
		event.preventDefault();
		selectLane(nextLane, true);
	}

	async function saveCandidate(candidate: RepositoryCandidate) {
		const fullName = candidate.repository.fullName;
		activeSaveFullName = fullName;
		saveErrors = { ...saveErrors, [fullName]: '' };
		try {
			await saveMutation.mutateAsync(fullName);
		} catch (error) {
			saveErrors = {
				...saveErrors,
				[fullName]: error instanceof Error ? error.message : 'The repository could not be saved.'
			};
		} finally {
			if (activeSaveFullName === fullName) activeSaveFullName = null;
		}
	}

	function cacheLabel(status: UserNeighborhood['cacheStatus']) {
		return {
			FRESH: 'Fresh cache',
			STALE: 'Cached result',
			REFRESHING: 'Refreshing',
			REFRESH_FAILED: 'Cached · refresh failed'
		}[status];
	}

	function cacheTone(status: UserNeighborhood['cacheStatus']) {
		return status === 'FRESH'
			? 'fresh'
			: status === 'REFRESH_FAILED'
				? 'failed'
				: status === 'REFRESHING'
					? 'refreshing'
					: 'stale';
	}
</script>

<svelte:head>
	<title>{login ? `@${login} · GitExplore` : 'Explore · GitExplore'}</title>
</svelte:head>

<div class="explore-page">
	<div class="utility-row">
		<a href="/app/explore" class="back-link"><ArrowLeft aria-hidden="true" size={16} /> Find another node</a>
		{#if neighborhood}
			<span class={`cache-chip ${cacheTone(neighborhood.cacheStatus)}`}>{cacheLabel(neighborhood.cacheStatus)}</span>
		{/if}
	</div>

	{#if trail.length}<GraphTrail {trail} />{/if}

	{#if !validLogin}
		<section class="state-card error" role="alert">
			<AlertCircle aria-hidden="true" size={22} />
			<div><h1>That GitHub username is not valid.</h1><p>Try a username, @handle, or profile URL from the explorer.</p></div>
			<a href="/app/explore">Return to search</a>
		</section>
	{:else if neighborhoodQuery.isPending}
		<section class="loading-card" aria-busy="true" aria-live="polite">
			<div class="orbit" aria-hidden="true"><span></span><Compass size={22} /></div>
			<div>
				<h1>{loadingPhase === 'expanding' ? `Mapping @${login}` : `Opening @${login}`}</h1>
				<p>{loadingPhase === 'expanding' ? 'Collecting this public neighborhood for the first time.' : 'Checking the shared graph cache.'}</p>
			</div>
		</section>
	{:else if neighborhoodQuery.isError}
		<section class="state-card error" role="alert">
			<AlertCircle aria-hidden="true" size={22} />
			<div>
				<h1>We could not open @{login}.</h1>
				<p>{neighborhoodQuery.error instanceof Error ? neighborhoodQuery.error.message : 'The graph request failed.'}</p>
			</div>
			<button type="button" onclick={() => neighborhoodQuery.refetch()}>Try again</button>
		</section>
	{:else if neighborhood}
		<section class="node-header" aria-labelledby="node-title" aria-busy={expandingCurrent || neighborhoodQuery.isFetching}>
			<div class="node-identity">
				{#if neighborhood.user.avatarUrl}
					<img src={neighborhood.user.avatarUrl} alt="" width="84" height="84" decoding="async" />
				{:else}
					<span class="avatar-fallback" aria-hidden="true">{(neighborhood.user.name || neighborhood.user.login).slice(0, 1).toUpperCase()}</span>
				{/if}
				<div>
					<p class="eyebrow">Current node</p>
					<h1 id="node-title">{neighborhood.user.name || neighborhood.user.login}</h1>
					<p class="handle">@{neighborhood.user.login}</p>
					{#if neighborhood.user.bio}<p class="bio">{neighborhood.user.bio}</p>{/if}
				</div>
			</div>

			<div class="node-actions">
				<a href={neighborhood.user.url} target="_blank" rel="noreferrer">
					GitHub <ExternalLink aria-hidden="true" size={15} /><span class="sr-only"> (opens in a new tab)</span>
				</a>
				<button type="button" disabled={expandingCurrent} onclick={refreshNeighborhood}>
					<RefreshCcw aria-hidden="true" size={16} class={expandingCurrent ? 'spin' : ''} />
					{expandingCurrent ? 'Refreshing…' : 'Refresh node'}
				</button>
			</div>

			<div class="node-stats">
				<div><strong>{(neighborhood.user.followersCount ?? neighborhood.followers.length).toLocaleString()}</strong><span>followers</span></div>
				<div><strong>{(neighborhood.user.followingCount ?? neighborhood.following.length).toLocaleString()}</strong><span>following</span></div>
				<div><strong>{neighborhood.repositories.length.toLocaleString()}</strong><span>repo signals</span></div>
				<div class="expedition-stamp"><GitBranch aria-hidden="true" size={17} /><span><strong>{expeditionLabel}</strong><small>depth {Math.max(0, trail.length - 1)}</small></span></div>
			</div>
		</section>

		{#if expandingCurrent}
			<div class="notice info" role="status" aria-live="polite">
				<GitBranch aria-hidden="true" size={17} />
				<span>{expansionMode === 'automatic' ? `Mapping @${login} for the first time. Cached context stays available.` : `Refreshing @${login}. Cached context stays available.`}</span>
			</div>
		{:else if expandMutation.isError && expandMutation.variables?.toLowerCase() === login.toLowerCase()}
			<div class="notice error" role="alert">
				<AlertCircle aria-hidden="true" size={17} />
				<span>{expandMutation.error instanceof Error ? expandMutation.error.message : 'The refresh failed. Cached results are still shown.'}</span>
				<button type="button" onclick={refreshNeighborhood}>Retry</button>
			</div>
		{/if}

		{#if partialCollections.length}
			<div class="notice warning" role="status">
				<AlertCircle aria-hidden="true" size={17} />
				<span>GitHub capped this refresh for {partialCollections.join(', ')}. Earlier cached relationships were preserved.</span>
			</div>
		{/if}

		<div class="atlas-layout">
			<aside class="connections-column" aria-label="Connections">
				<div class="lane-tabs" role="tablist" aria-label="Connection direction">
					<button
						id="followers-tab"
						type="button"
						role="tab"
						aria-selected={activeLane === 'followers'}
						aria-controls="followers-panel"
						tabindex={activeLane === 'followers' ? 0 : -1}
						onclick={() => selectLane('followers')}
						onkeydown={handleLaneKeydown}
					>
						Followers <span>{neighborhood.followers.length}</span>
					</button>
					<button
						id="following-tab"
						type="button"
						role="tab"
						aria-selected={activeLane === 'following'}
						aria-controls="following-panel"
						tabindex={activeLane === 'following' ? 0 : -1}
						onclick={() => selectLane('following')}
						onkeydown={handleLaneKeydown}
					>
						Following <span>{neighborhood.following.length}</span>
					</button>
				</div>
				<div
					id="followers-panel"
					role="tabpanel"
					aria-labelledby="followers-tab"
					hidden={activeLane !== 'followers'}
				>
					<PersonLane
						title="Followers"
						description="People who follow this account."
						people={neighborhood.followers}
						{trail}
						initialVisible={10}
					/>
				</div>
				<div
					id="following-panel"
					role="tabpanel"
					aria-labelledby="following-tab"
					hidden={activeLane !== 'following'}
				>
					<PersonLane
						title="Following"
						description="People this account follows."
						people={neighborhood.following}
						{trail}
						initialVisible={10}
					/>
				</div>
			</aside>

			<div class="discovery-column">
				{#if userInsightsQuery.isPending}
					<section class="insight-skeleton" aria-busy="true" aria-live="polite">
						<span class="sr-only">Loading recent public commit activity.</span>
						<div aria-hidden="true"></div><div aria-hidden="true"></div><div aria-hidden="true"></div>
					</section>
				{:else if userInsightsQuery.data}
					<UserCommitList insight={userInsightsQuery.data} />
				{:else if userInsightsQuery.isError}
					<div class="notice error" role="alert"><AlertCircle aria-hidden="true" size={17} /><span>Recent public commit activity is unavailable right now.</span></div>
				{/if}

				<section aria-labelledby="discoveries-title" class="discoveries">
					<header>
						<div>
							<p class="eyebrow"><Sparkles aria-hidden="true" size={15} /> Ranked discoveries</p>
							<h2 id="discoveries-title">Repositories with uncommon local signal</h2>
							<p>Nearby endorsements, reach, and fresh activity—ranked so the reason is visible before the star count.</p>
						</div>
						<span><Users aria-hidden="true" size={15} /> {neighborhood.repositories.length} found</span>
					</header>

					{#if visibleRepositories.length}
						<div class="repository-grid">
							{#each visibleRepositories as candidate (candidate.repository.fullName)}
								<RepositoryDiscoveryCard
									{candidate}
									saving={saveMutation.isPending && activeSaveFullName === candidate.repository.fullName}
									saveError={saveErrors[candidate.repository.fullName] || null}
									onSave={saveCandidate}
								/>
							{/each}
						</div>
						{#if hiddenRepositoryCount}
							<button class="show-more" type="button" onclick={() => (visibleRepositoryCount += 8)}>
								Show {Math.min(8, hiddenRepositoryCount)} more discoveries
							</button>
						{/if}
					{:else}
						<div class="empty-discoveries"><Sparkles aria-hidden="true" size={22} /><h3>No repository signals yet</h3><p>Refresh this node or continue through a connection to uncover more.</p></div>
					{/if}
				</section>
			</div>
		</div>
	{/if}
</div>

<style>
	.explore-page { display: grid; gap: var(--space-4); }
	.utility-row { display: flex; min-height: var(--control-hit-target); align-items: center; justify-content: space-between; gap: var(--space-3); }
	.back-link { display: inline-flex; min-height: var(--control-hit-target); align-items: center; gap: var(--space-2); border-radius: var(--radius-md); padding-inline: var(--space-2); color: var(--muted-foreground); font-size: var(--type-size-sm); font-weight: 650; text-decoration: none; }
	.back-link:hover { background: var(--muted); color: var(--foreground); }
	.cache-chip { display: inline-flex; min-height: 2rem; align-items: center; border-radius: var(--radius-pill); padding-inline: var(--space-3); font-size: var(--type-size-xs); font-weight: 700; }
	.cache-chip.fresh { background: var(--success-muted); color: var(--success-muted-foreground); }
	.cache-chip.stale { background: var(--warning-muted); color: var(--warning-muted-foreground); }
	.cache-chip.refreshing { background: var(--info-muted); color: var(--info-muted-foreground); }
	.cache-chip.failed { background: var(--destructive-muted); color: var(--destructive-muted-foreground); }

	.node-header { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--space-5); overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-xl); background: var(--card); padding: var(--space-6); box-shadow: var(--shadow-card); }
	.node-identity { display: flex; min-width: 0; align-items: flex-start; gap: var(--space-4); }
	.node-identity img, .avatar-fallback { width: 5.25rem; height: 5.25rem; flex: 0 0 auto; border: 1px solid var(--border); border-radius: var(--radius-xl); background: var(--surface-inset); object-fit: cover; }
	.avatar-fallback { display: grid; place-items: center; color: var(--accent-foreground); font-size: var(--type-size-2xl); font-weight: 700; }
	.node-identity h1 { overflow-wrap: anywhere; margin: var(--space-2) 0 0; font-size: clamp(2rem, 4vw, 3.25rem); font-weight: 680; letter-spacing: -0.05em; line-height: 0.98; }
	.handle { margin: var(--space-2) 0 0; color: var(--primary); font-family: var(--font-mono); font-size: var(--type-size-sm); }
	.bio { max-width: 48rem; margin: var(--space-3) 0 0; color: var(--muted-foreground); font-size: var(--type-size-sm); line-height: 1.55; }
	.node-actions { display: flex; flex-wrap: wrap; align-items: flex-start; justify-content: flex-end; gap: var(--space-2); }
	.node-actions a, .node-actions button, .state-card a, .state-card button, .notice button { display: inline-flex; min-height: var(--control-hit-target); align-items: center; justify-content: center; gap: var(--space-2); border: 1px solid var(--border-strong); border-radius: var(--radius-md); background: var(--surface); padding-inline: var(--space-4); color: var(--foreground); font: inherit; font-size: var(--type-size-sm); font-weight: 700; text-decoration: none; cursor: pointer; }
	.node-actions button { border-color: var(--primary); background: var(--primary); color: var(--primary-foreground); }
	.node-actions button:disabled { cursor: wait; opacity: var(--effect-disabled-opacity); }
	.spin { animation: spin 900ms linear infinite; }
	.node-stats { grid-column: 1 / -1; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)) minmax(10rem, auto); gap: var(--space-2); padding-top: var(--space-5); border-top: 1px solid var(--border); }
	.node-stats > div { min-width: 0; padding-inline: var(--space-2); }
	.node-stats strong, .node-stats span { display: block; }
	.node-stats > div > strong { font-family: var(--font-mono); font-size: var(--type-size-xl); }
	.node-stats > div > span { margin-top: var(--space-1); color: var(--muted-foreground); font-size: var(--type-size-xs); }
	.node-stats .expedition-stamp { display: flex; align-items: center; gap: var(--space-2); border-left: 1px solid var(--border); color: var(--primary); }
	.expedition-stamp span { margin: 0; color: inherit; }
	.expedition-stamp strong { font-size: var(--type-size-sm); }
	.expedition-stamp small { display: block; margin-top: var(--space-1); color: var(--muted-foreground); font-family: var(--font-mono); font-size: 0.68rem; }

	.notice { display: flex; min-height: var(--control-hit-target); align-items: center; gap: var(--space-3); border: 1px solid; border-radius: var(--radius-lg); padding: var(--space-3) var(--space-4); font-size: var(--type-size-sm); }
	.notice > :global(svg) { flex: 0 0 auto; }
	.notice span { flex: 1; }
	.notice.info { border-color: var(--info); background: var(--info-muted); color: var(--info-muted-foreground); }
	.notice.warning { border-color: var(--warning); background: var(--warning-muted); color: var(--warning-muted-foreground); }
	.notice.error { border-color: var(--destructive); background: var(--destructive-muted); color: var(--destructive-muted-foreground); }

	.atlas-layout { display: grid; gap: var(--space-4); align-items: start; }
	.connections-column { min-width: 0; }
	.lane-tabs { display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-1); margin-bottom: var(--space-2); border-radius: var(--radius-lg); background: var(--muted); padding: var(--space-1); }
	.lane-tabs button { display: flex; min-height: var(--control-hit-target); align-items: center; justify-content: center; gap: var(--space-2); border: 0; border-radius: var(--radius-md); background: transparent; color: var(--muted-foreground); font: inherit; font-size: var(--type-size-sm); font-weight: 700; cursor: pointer; }
	.lane-tabs button[aria-selected='true'] { background: var(--surface); color: var(--foreground); box-shadow: inset 0 0 0 1px var(--primary), var(--shadow-hairline); }
	.lane-tabs span { font-family: var(--font-mono); font-size: var(--type-size-xs); }
	.discovery-column { display: grid; min-width: 0; gap: var(--space-6); }

	.discoveries > header { display: flex; align-items: flex-end; justify-content: space-between; gap: var(--space-4); margin-bottom: var(--space-4); }
	.discoveries .eyebrow { display: flex; align-items: center; gap: var(--space-2); color: var(--primary); }
	.discoveries h2 { margin: var(--space-2) 0 0; font-size: clamp(1.65rem, 3vw, 2.25rem); font-weight: 680; letter-spacing: var(--type-tracking-heading); line-height: 1.05; }
	.discoveries header p:last-child { max-width: 44rem; margin: var(--space-2) 0 0; color: var(--muted-foreground); font-size: var(--type-size-sm); line-height: 1.55; }
	.discoveries header > span { display: inline-flex; min-height: 2rem; flex: 0 0 auto; align-items: center; gap: var(--space-1); border-radius: var(--radius-pill); background: var(--muted); padding-inline: var(--space-3); color: var(--muted-foreground); font-size: var(--type-size-xs); font-weight: 650; }
	.repository-grid { display: grid; gap: var(--space-4); }
	.show-more { display: flex; min-height: var(--control-hit-target); width: 100%; align-items: center; justify-content: center; margin-top: var(--space-4); border: 1px dashed var(--border-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--primary); font: inherit; font-size: var(--type-size-sm); font-weight: 700; cursor: pointer; }
	.show-more:hover { background: var(--muted); }
	.empty-discoveries { display: grid; place-items: center; border: 1px dashed var(--border-strong); border-radius: var(--radius-xl); background: var(--surface); padding: var(--space-8); color: var(--muted-foreground); text-align: center; }
	.empty-discoveries h3 { margin: var(--space-3) 0 0; color: var(--foreground); }
	.empty-discoveries p { margin: var(--space-1) 0 0; font-size: var(--type-size-sm); }

	.loading-card, .state-card { display: flex; min-height: 22rem; align-items: center; justify-content: center; gap: var(--space-5); border: 1px solid var(--border); border-radius: var(--radius-xl); background: var(--card); padding: var(--space-8); box-shadow: var(--shadow-card); text-align: left; }
	.loading-card h1, .state-card h1 { margin: 0; font-size: var(--type-size-2xl); }
	.loading-card p, .state-card p { margin: var(--space-2) 0 0; color: var(--muted-foreground); font-size: var(--type-size-sm); }
	.state-card { flex-wrap: wrap; }
	.state-card.error > :global(svg) { color: var(--destructive); }
	.orbit { position: relative; display: grid; width: 4rem; height: 4rem; flex: 0 0 auto; place-items: center; border: 1px dashed var(--primary); border-radius: var(--radius-pill); color: var(--primary); }
	.orbit span { position: absolute; width: 0.55rem; height: 0.55rem; border-radius: var(--radius-pill); background: var(--success); animation: orbit 1.8s linear infinite; }
	.insight-skeleton { display: grid; gap: 1px; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-xl); background: var(--border); }
	.insight-skeleton div { height: 4.5rem; background: linear-gradient(100deg, var(--card) 30%, var(--muted) 50%, var(--card) 70%); background-size: 220% 100%; animation: shimmer 1.4s infinite; }

	@keyframes spin { to { transform: rotate(360deg); } }
	@keyframes orbit { from { transform: rotate(0deg) translateX(2rem) rotate(0deg); } to { transform: rotate(360deg) translateX(2rem) rotate(-360deg); } }
	@keyframes shimmer { to { background-position-x: -220%; } }

	@media (min-width: 64rem) {
		.atlas-layout { grid-template-columns: minmax(18rem, 20rem) minmax(0, 1fr); }
		.connections-column { position: sticky; top: 5rem; max-height: calc(100svh - 6rem); overflow-y: auto; scrollbar-width: thin; }
	}

	@media (min-width: 80rem) {
		.repository-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
	}

	@media (max-width: 44rem) {
		.node-header { grid-template-columns: 1fr; padding: var(--space-4); }
		.node-identity img, .avatar-fallback { width: 4rem; height: 4rem; border-radius: var(--radius-lg); }
		.node-actions { justify-content: flex-start; }
		.node-stats { grid-template-columns: repeat(3, minmax(0, 1fr)); }
		.node-stats > div > strong { font-size: var(--type-size-lg); }
		.node-stats .expedition-stamp { grid-column: 1 / -1; border-top: 1px solid var(--border); border-left: 0; padding-top: var(--space-3); }
		.discoveries > header { align-items: flex-start; flex-direction: column; }
	}

	@media (prefers-reduced-motion: reduce) {
		.spin, .orbit span, .insight-skeleton div { animation: none; }
	}
</style>
