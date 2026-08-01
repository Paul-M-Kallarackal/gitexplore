<script lang="ts">
	import type { GraphUser } from '@gitexplore/api-client';
	import { ArrowRight, Search, UsersRound } from 'lucide-svelte';
	import { buildExploreHref } from '$lib/graph-navigation';

	let {
		title,
		description,
		people,
		trail,
		initialVisible = 8,
		onPrefetch
	}: {
		title: string;
		description: string;
		people: GraphUser[];
		trail: string[];
		initialVisible?: number;
		onPrefetch?: (person: GraphUser) => void;
	} = $props();

	let filter = $state('');
	let revealPages = $state(0);

	const laneId = $derived(title.toLowerCase().replace(/[^a-z0-9]+/g, '-'));
	const normalizedFilter = $derived(filter.trim().toLowerCase());
	const filteredPeople = $derived.by(() => {
		if (!normalizedFilter) return people;
		return people.filter((person) =>
			`${person.name ?? ''} ${person.login}`.toLowerCase().includes(normalizedFilter)
		);
	});
	const visibleLimit = $derived(Math.max(1, initialVisible) * (revealPages + 1));
	const visiblePeople = $derived(filteredPeople.slice(0, visibleLimit));
	const hiddenCount = $derived(Math.max(0, filteredPeople.length - visiblePeople.length));

	$effect(() => {
		normalizedFilter;
		revealPages = 0;
	});

	function initial(person: GraphUser) {
		return (person.name || person.login).slice(0, 1).toUpperCase();
	}

	function showMore() {
		revealPages += 1;
	}

	function clearFilter() {
		filter = '';
	}
</script>

<section
	aria-labelledby={`${laneId}-title`}
	class="overflow-hidden rounded-[var(--radius-xl)] border border-[var(--border)] bg-[var(--card)] shadow-[var(--shadow-soft)]"
>
	<header class="border-b border-[var(--border)] px-[var(--space-4)] py-[var(--space-4)] sm:px-[var(--space-5)]">
		<div class="flex items-start justify-between gap-[var(--space-3)]">
			<div class="flex min-w-0 gap-[var(--space-3)]">
				<span
					aria-hidden="true"
					class="grid size-10 shrink-0 place-items-center rounded-[var(--radius-md)] bg-[var(--accent)] text-[var(--accent-foreground)]"
				>
					<UsersRound size={17} />
				</span>
				<div class="min-w-0">
					<h2
						id={`${laneId}-title`}
						class="text-base font-semibold tracking-[var(--type-tracking-heading)] sm:text-lg"
					>
						{title}
					</h2>
					<p id={`${laneId}-description`} class="mt-[var(--space-1)] text-sm text-[var(--muted-foreground)]">
						{description}
					</p>
				</div>
			</div>
			<span
				class="inline-flex min-h-8 min-w-8 shrink-0 items-center justify-center rounded-[var(--radius-pill)] bg-[var(--muted)] px-[var(--space-2)] font-[var(--font-mono)] text-xs font-medium text-[var(--muted-foreground)]"
			>
				<span aria-hidden="true">{people.length}</span>
				<span class="sr-only">{people.length} {title.toLowerCase()} loaded</span>
			</span>
		</div>

		{#if people.length > initialVisible}
			<label class="relative mt-[var(--space-3)] block" for={`${laneId}-filter`}>
				<span class="sr-only">Filter {title.toLowerCase()}</span>
				<Search
					aria-hidden="true"
					size={15}
					class="pointer-events-none absolute left-[var(--space-3)] top-1/2 -translate-y-1/2 text-[var(--muted-foreground)]"
				/>
				<input
					id={`${laneId}-filter`}
					type="search"
					value={filter}
					oninput={(event) => (filter = event.currentTarget.value)}
					autocomplete="off"
					placeholder={`Filter ${title.toLowerCase()}`}
					class="min-h-[var(--control-hit-target)] w-full rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--input)_65%,var(--foreground))] bg-[var(--surface-inset)] pl-9 pr-[var(--space-3)] text-sm text-[var(--foreground)] outline-none transition-[border-color,box-shadow,background-color] duration-[var(--motion-duration-fast)] placeholder:text-[var(--muted-foreground)] hover:border-[var(--ring)] focus:border-[var(--ring)] focus:bg-[var(--surface)] focus:shadow-[var(--shadow-focus)] motion-reduce:transition-none"
				/>
			</label>
			<span class="sr-only" role="status" aria-live="polite" aria-atomic="true">
				{normalizedFilter
					? `${filteredPeople.length} matching ${title.toLowerCase()}`
					: `${people.length} ${title.toLowerCase()} available`}
			</span>
		{/if}
	</header>

	{#if visiblePeople.length}
		<ul class="divide-y divide-[var(--border)]" role="list" aria-describedby={`${laneId}-description`}>
			{#each visiblePeople as person (person.login)}
				<li>
					<a
						href={buildExploreHref(person.login, trail)}
						aria-label={person.name ? `Explore ${person.name} (@${person.login})` : `Explore @${person.login}`}
						onpointerenter={() => onPrefetch?.(person)}
						onfocus={() => onPrefetch?.(person)}
						class="group flex min-h-14 items-center gap-[var(--space-3)] px-[var(--space-4)] py-[var(--space-2)] transition-colors duration-[var(--motion-duration-fast)] hover:bg-[var(--muted)] motion-reduce:transition-none sm:px-[var(--space-5)]"
					>
						{#if person.avatarUrl}
							<img
								src={person.avatarUrl}
								alt=""
								width="36"
								height="36"
								loading="lazy"
								decoding="async"
								class="size-9 shrink-0 rounded-[var(--radius-pill)] bg-[var(--surface-inset)] object-cover shadow-[0_0_0_1px_var(--effect-image-outline)]"
							/>
						{:else}
							<span
								aria-hidden="true"
								class="grid size-9 shrink-0 place-items-center rounded-[var(--radius-pill)] bg-[var(--accent)] text-sm font-semibold text-[var(--accent-foreground)]"
							>
								{initial(person)}
							</span>
						{/if}
						<span class="min-w-0 flex-1">
							<span class="block truncate text-sm font-semibold">
								{person.name || person.login}
							</span>
							<span class="mt-0.5 block truncate text-xs text-[var(--muted-foreground)]">
								@{person.login}
							</span>
						</span>
						<span
							aria-hidden="true"
							class="grid size-8 shrink-0 place-items-center rounded-[var(--radius-md)] text-[var(--muted-foreground)] transition-[background-color,color,transform] duration-[var(--motion-duration-fast)] group-hover:translate-x-0.5 group-hover:bg-[var(--surface)] group-hover:text-[var(--primary)] motion-reduce:transform-none motion-reduce:transition-none"
						>
							<ArrowRight size={15} />
						</span>
					</a>
				</li>
			{/each}
		</ul>

		{#if hiddenCount > 0}
			<div class="border-t border-[var(--border)] p-[var(--space-2)]">
				<button
					type="button"
					onclick={showMore}
					class="inline-flex min-h-[var(--control-hit-target)] w-full items-center justify-center rounded-[var(--radius-md)] px-[var(--space-3)] text-sm font-semibold text-[var(--primary)] transition-colors duration-[var(--motion-duration-fast)] hover:bg-[var(--muted)] motion-reduce:transition-none"
				>
					Show {Math.min(Math.max(1, initialVisible), hiddenCount)} more
				</button>
			</div>
		{/if}
	{:else if normalizedFilter}
		<div class="px-[var(--space-5)] py-[var(--space-8)] text-center">
			<p class="text-sm font-semibold">No matching accounts</p>
			<p class="mt-[var(--space-1)] text-sm text-[var(--muted-foreground)]">
				Try another name or GitHub handle.
			</p>
			<button
				type="button"
				onclick={clearFilter}
				class="mt-[var(--space-3)] min-h-[var(--control-hit-target)] rounded-[var(--radius-md)] px-[var(--space-3)] text-sm font-semibold text-[var(--primary)] hover:bg-[var(--muted)]"
			>
				Clear filter
			</button>
		</div>
	{:else}
		<div class="px-[var(--space-5)] py-[var(--space-8)] text-center">
			<p class="text-sm font-semibold">No known connections in this lane.</p>
			<p class="mt-[var(--space-1)] text-sm text-[var(--muted-foreground)]">
				A refresh may uncover more of this public graph.
			</p>
		</div>
	{/if}
</section>
