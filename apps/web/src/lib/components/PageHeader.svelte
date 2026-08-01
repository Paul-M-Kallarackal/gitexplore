<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		eyebrow,
		title,
		description,
		actions
	}: {
		eyebrow: string;
		title: string;
		description: string;
		actions?: Snippet;
	} = $props();
</script>

<div class="mb-6 flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
	<div class="max-w-2xl">
		<p class="eyebrow">{eyebrow}</p>
		<h1 class="page-title mt-3 font-[var(--font-display)]">{title}</h1>
		<p class="mt-3 max-w-prose text-sm leading-6 text-[var(--muted-foreground)]">{description}</p>
	</div>
	{#if actions}
		<div class="flex items-center gap-2 lg:justify-end">
			{@render actions()}
		</div>
	{/if}
</div>
