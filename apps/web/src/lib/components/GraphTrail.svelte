<script lang="ts">
	import { ChevronRight, GitBranch, MoreHorizontal } from 'lucide-svelte';
	import { buildTrailHref } from '$lib/graph-navigation';

	let { trail, maxVisible = 6 }: { trail: string[]; maxVisible?: number } = $props();
	let expanded = $state(false);

	type TrailStep = { login: string; index: number };

	const steps = $derived(trail.map((login, index) => ({ login, index })));
	const tailCount = $derived(Math.max(2, maxVisible - 1));
	const hiddenSteps = $derived.by((): TrailStep[] => {
		if (steps.length <= tailCount + 1) return [];
		return steps.slice(1, steps.length - tailCount);
	});
	const visibleSteps = $derived.by((): TrailStep[] => {
		if (expanded || hiddenSteps.length === 0) return steps;
		return [steps[0], ...steps.slice(-tailCount)].filter(
			(step): step is TrailStep => Boolean(step)
		);
	});
	const hopCount = $derived(Math.max(0, trail.length - 1));
</script>

<nav
	aria-label="Exploration trail"
	class="rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface)] shadow-[var(--shadow-soft)]"
>
	<div class="flex min-h-[var(--control-hit-target)] items-center justify-between gap-[var(--space-3)] border-b border-[var(--border)] px-[var(--space-3)] sm:px-[var(--space-4)]">
		<div class="flex min-w-0 items-center gap-[var(--space-2)]">
			<span
				aria-hidden="true"
				class="grid size-8 shrink-0 place-items-center rounded-[var(--radius-pill)] bg-[var(--accent)] text-[var(--accent-foreground)]"
			>
				<GitBranch size={15} />
			</span>
			<span class="truncate text-sm font-semibold">Expedition trail</span>
			<span class="shrink-0 font-[var(--font-mono)] text-xs text-[var(--muted-foreground)]">
				{hopCount} {hopCount === 1 ? 'hop' : 'hops'}
			</span>
		</div>

		{#if hiddenSteps.length}
			<button
				type="button"
				onclick={() => (expanded = !expanded)}
				aria-expanded={expanded}
				aria-controls="exploration-trail-list"
				class="inline-flex min-h-[var(--control-hit-target)] shrink-0 items-center gap-[var(--space-1)] rounded-[var(--radius-md)] px-[var(--space-2)] text-xs font-semibold text-[var(--primary)] transition-colors duration-[var(--motion-duration-fast)] hover:bg-[var(--muted)] motion-reduce:transition-none"
			>
				<MoreHorizontal aria-hidden="true" size={15} />
				{expanded ? 'Collapse' : `${hiddenSteps.length} earlier`}
			</button>
		{/if}
	</div>

	<div class="max-w-full overflow-x-auto">
		<ol id="exploration-trail-list" class="flex min-w-max items-center px-[var(--space-2)]" role="list">
			{#each visibleSteps as step, position (`${step.login}-${step.index}`)}
				{#if position > 0}
					<li aria-hidden="true" class="flex items-center text-[var(--muted-foreground)]">
						<ChevronRight size={14} />
					</li>
					{#if !expanded && hiddenSteps.length && position === 1}
						<li
							aria-hidden="true"
							class="mx-[var(--space-1)] inline-flex h-7 min-w-7 items-center justify-center rounded-[var(--radius-pill)] bg-[var(--surface-inset)] px-[var(--space-2)] font-[var(--font-mono)] text-xs text-[var(--muted-foreground)]"
						>
							+{hiddenSteps.length}
						</li>
						<li aria-hidden="true" class="flex items-center text-[var(--muted-foreground)]">
							<ChevronRight size={14} />
						</li>
					{/if}
				{/if}
				<li>
					<a
						href={buildTrailHref(trail, step.index)}
						aria-current={step.index === trail.length - 1 ? 'page' : undefined}
						class={`inline-flex min-h-[var(--control-hit-target)] items-center rounded-[var(--radius-md)] px-[var(--space-3)] text-sm font-medium transition-colors duration-[var(--motion-duration-fast)] motion-reduce:transition-none ${
							step.index === trail.length - 1
								? 'bg-[var(--accent)] text-[var(--accent-foreground)] shadow-[inset_0_0_0_1px_var(--primary)]'
								: 'text-[var(--muted-foreground)] hover:bg-[var(--muted)] hover:text-[var(--foreground)]'
						}`}
					>
						@{step.login}
					</a>
				</li>
			{/each}
		</ol>
	</div>
</nav>
