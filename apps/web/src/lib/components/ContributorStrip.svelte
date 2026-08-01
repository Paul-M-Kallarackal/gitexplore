<script lang="ts">
	import type { RepositoryContributorInsights } from '@gitexplore/api-client';
	import { ArrowRight, UsersRound } from 'lucide-svelte';

	let { insight }: { insight: RepositoryContributorInsights } = $props();
</script>

<section class="contributors" aria-labelledby="contributors-title">
	<header>
		<div>
			<p class="eyebrow">Active contributors</p>
			<h2 id="contributors-title">People moving this repository</h2>
		</div>
		<span><UsersRound aria-hidden="true" size={15} /> {insight.contributors.length} loaded</span>
	</header>

	{#if insight.contributors.length}
		<ul role="list">
			{#each insight.contributors as contributor (contributor.login)}
				<li>
					<a href={`/app/explore/${encodeURIComponent(contributor.login)}`}>
						{#if contributor.avatarUrl}
							<img src={contributor.avatarUrl} alt="" width="52" height="52" loading="lazy" decoding="async" />
						{:else}
							<span class="avatar-fallback" aria-hidden="true">{contributor.login.slice(0, 1).toUpperCase()}</span>
						{/if}
						<span class="name"><strong>@{contributor.login}</strong><small>{contributor.contributions.toLocaleString()} contributions</small></span>
						<ArrowRight aria-hidden="true" size={16} />
					</a>
				</li>
			{/each}
		</ul>
	{:else}
		<div class="empty"><UsersRound aria-hidden="true" size={22} /><p>No public contributors were returned.</p></div>
	{/if}

	<footer>
		<span>{insight.sourceDescription}</span>
		<span>{insight.cacheStatus.toLowerCase().replace('_', ' ')}</span>
	</footer>
</section>

<style>
	.contributors {
		overflow: hidden;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: var(--card);
		box-shadow: var(--shadow-soft);
	}

	header {
		display: flex;
		align-items: flex-end;
		justify-content: space-between;
		gap: var(--space-4);
		padding: var(--space-5);
		border-bottom: 1px solid var(--border);
	}

	header h2 {
		margin: var(--space-2) 0 0;
		font-size: var(--type-size-xl);
		font-weight: 670;
		letter-spacing: var(--type-tracking-heading);
	}

	header > span {
		display: inline-flex;
		min-height: 2rem;
		align-items: center;
		gap: var(--space-1);
		border-radius: var(--radius-pill);
		background: var(--muted);
		padding-inline: var(--space-3);
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
		font-weight: 650;
		white-space: nowrap;
	}

	ul {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 15rem), 1fr));
		gap: 1px;
		margin: 0;
		padding: 1px;
		background: var(--border);
		list-style: none;
	}

	li {
		background: var(--card);
	}

	li a {
		display: grid;
		min-height: 4.75rem;
		grid-template-columns: 3.25rem minmax(0, 1fr) auto;
		gap: var(--space-3);
		align-items: center;
		padding: var(--space-3) var(--space-4);
		color: var(--foreground);
		text-decoration: none;
		transition: background-color var(--motion-duration-fast) var(--motion-ease-standard);
	}

	li a:hover {
		background: var(--muted);
	}

	li img,
	.avatar-fallback {
		width: 3.25rem;
		height: 3.25rem;
		border-radius: var(--radius-lg);
		background: var(--surface-inset);
		object-fit: cover;
	}

	.avatar-fallback {
		display: grid;
		place-items: center;
		color: var(--accent-foreground);
		font-weight: 700;
	}

	.name {
		min-width: 0;
	}

	.name strong,
	.name small {
		display: block;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.name strong {
		font-size: var(--type-size-sm);
	}

	.name small {
		margin-top: var(--space-1);
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
	}

	li :global(svg) {
		color: var(--muted-foreground);
	}

	.empty {
		display: grid;
		place-items: center;
		gap: var(--space-2);
		padding: var(--space-8);
		color: var(--muted-foreground);
		font-size: var(--type-size-sm);
	}

	.empty p {
		margin: 0;
	}

	footer {
		display: flex;
		justify-content: space-between;
		gap: var(--space-4);
		padding: var(--space-3) var(--space-5);
		border-top: 1px solid var(--border);
		background: var(--surface-inset);
		color: var(--muted-foreground);
		font-size: 0.68rem;
	}

	@media (max-width: 36rem) {
		header {
			align-items: flex-start;
			flex-direction: column;
		}
	}
</style>
