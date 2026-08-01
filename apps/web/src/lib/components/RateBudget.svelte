<script lang="ts">
	import type { GitHubRateLimit } from '@gitexplore/api-client';
	import { Gauge } from 'lucide-svelte';

	let {
		rate,
		pending = false,
		compact = false
	}: {
		rate?: GitHubRateLimit | null;
		pending?: boolean;
		compact?: boolean;
	} = $props();
	const protectedReserve = 1_000;

	const remainingPercent = $derived(
		rate && rate.limit > 0 ? Math.max(0, Math.min(100, (rate.remaining / rate.limit) * 100)) : 0
	);
	const tone = $derived(
		!rate || remainingPercent > 35
			? 'healthy'
			: rate.remaining > protectedReserve
				? 'watch'
				: 'low'
	);

	function resetLabel(value: string) {
		const date = new Date(value);
		if (!Number.isFinite(date.getTime())) return 'reset time unknown';
		return `resets ${date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`;
	}
</script>

<div
	class:compact
	class={`rate-budget ${tone}`}
	role="status"
	aria-label={rate
		? `${rate.remaining} of ${rate.limit} GitHub requests remaining, ${protectedReserve} request reserve protected, ${resetLabel(rate.resetAt)}`
		: 'GitHub request budget unavailable'}
>
	<span class="rate-icon" aria-hidden="true"><Gauge size={compact ? 15 : 17} /></span>
	<div class="rate-copy">
		<span class="rate-label">GitHub budget</span>
		<strong>{pending && !rate ? 'Checking…' : rate ? `${rate.remaining.toLocaleString()} / ${rate.limit.toLocaleString()}` : 'Unavailable'}</strong>
		{#if rate && !compact}<small>{resetLabel(rate.resetAt)} · {protectedReserve.toLocaleString()} protected</small>{/if}
	</div>
	{#if rate}
		<span class="meter" aria-hidden="true"><span style={`width: ${remainingPercent}%`}></span></span>
	{/if}
</div>

<style>
	.rate-budget {
		position: relative;
		display: grid;
		min-width: 12.5rem;
		min-height: var(--control-hit-target);
		grid-template-columns: auto 1fr;
		gap: var(--space-2);
		align-items: center;
		overflow: hidden;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: var(--surface);
		padding: var(--space-2) var(--space-3) calc(var(--space-2) + 3px);
		box-shadow: var(--shadow-hairline);
	}

	.rate-budget.compact {
		min-width: auto;
		grid-template-columns: auto auto;
		padding-inline: var(--space-2);
	}

	.rate-icon {
		display: grid;
		width: 1.75rem;
		height: 1.75rem;
		place-items: center;
		border-radius: var(--radius-sm);
		background: var(--success-muted);
		color: var(--success-muted-foreground);
	}

	.watch .rate-icon {
		background: var(--warning-muted);
		color: var(--warning-muted-foreground);
	}

	.low .rate-icon {
		background: var(--destructive-muted);
		color: var(--destructive-muted-foreground);
	}

	.rate-copy {
		display: grid;
		grid-template-columns: 1fr auto;
		gap: 0 var(--space-2);
		align-items: baseline;
		line-height: 1.15;
	}

	.rate-label {
		color: var(--muted-foreground);
		font-size: 0.68rem;
		font-weight: 700;
		letter-spacing: var(--type-tracking-caps);
		text-transform: uppercase;
	}

	.rate-copy strong {
		font-family: var(--font-mono);
		font-size: var(--type-size-xs);
		font-weight: 650;
	}

	.rate-copy small {
		grid-column: 1 / -1;
		margin-top: 0.18rem;
		color: var(--muted-foreground);
		font-size: 0.68rem;
	}

	.compact .rate-label,
	.compact small {
		display: none;
	}

	.compact .rate-copy {
		display: block;
	}

	.meter {
		position: absolute;
		right: 0;
		bottom: 0;
		left: 0;
		height: 3px;
		background: var(--surface-inset);
	}

	.meter span {
		display: block;
		height: 100%;
		background: var(--success);
		transition: width var(--motion-duration-base) var(--motion-ease-standard);
	}

	.watch .meter span {
		background: var(--warning);
	}

	.low .meter span {
		background: var(--destructive);
	}

	@media (max-width: 36rem) {
		.rate-budget:not(.compact) {
			min-width: auto;
		}

		.rate-budget:not(.compact) .rate-label,
		.rate-budget:not(.compact) small {
			display: none;
		}

		.rate-budget:not(.compact) .rate-copy {
			display: block;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.meter span {
			transition: none;
		}
	}
</style>
