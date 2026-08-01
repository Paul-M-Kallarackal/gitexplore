<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';
	import { cn } from '$ui/lib/cn';

	let {
		icon: Icon,
		label,
		class: className = '',
		disabled = false,
		children,
		...rest
	}: {
		icon: any;
		label: string;
		class?: string;
		disabled?: boolean;
		children?: Snippet;
	} & HTMLButtonAttributes = $props();
</script>

<button
	aria-label={label}
	title={label}
	class={cn(
		'inline-flex min-h-[var(--control-hit-target)] min-w-[var(--control-hit-target)] items-center justify-center rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--surface)] text-[var(--muted-foreground)] transition-[border-color,color,background-color] duration-[var(--motion-duration-fast)] hover:border-[var(--primary)] hover:bg-[var(--muted)] hover:text-[var(--foreground)] disabled:cursor-not-allowed disabled:opacity-[var(--effect-disabled-opacity)]',
		className
	)}
	{disabled}
	{...rest}
>
	<Icon aria-hidden="true" size={16} />
	<span class="sr-only">{label}</span>
	{@render children?.()}
</button>
