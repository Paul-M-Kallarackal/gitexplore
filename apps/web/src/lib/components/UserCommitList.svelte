<script lang="ts">
	import type { UserCommitRepositoryInsights } from '@gitexplore/api-client';
	import { ArrowRight, GitCommitHorizontal, Radio } from 'lucide-svelte';

	let { insight }: { insight: UserCommitRepositoryInsights } = $props();

	function detailHref(fullName: string) {
		const [owner, name] = fullName.split('/', 2);
		return owner && name
			? `/app/repository/${encodeURIComponent(owner)}/${encodeURIComponent(name)}`
			: `https://github.com/${fullName}`;
	}

	function lastPushLabel(value: string | null) {
		if (!value) return 'Recent activity';
		const date = new Date(value);
		return Number.isFinite(date.getTime())
			? `Last push ${date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}`
			: 'Recent activity';
	}
</script>

<section class="commit-list" aria-labelledby="commit-list-title">
	<header>
		<div>
			<p class="eyebrow">Public push activity</p>
			<h2 id="commit-list-title">Where @{insight.login} has been building</h2>
		</div>
		<span class="window"><Radio aria-hidden="true" size={14} /> {insight.windowDays} day window</span>
	</header>

	{#if insight.repositories.length}
		<ol role="list">
			{#each insight.repositories.slice(0, 8) as repository, index (repository.fullName)}
				<li>
					<a href={detailHref(repository.fullName)}>
						<span class="rank">{String(index + 1).padStart(2, '0')}</span>
						<img
							src={`https://github.com/${encodeURIComponent(repository.fullName.split('/')[0] ?? '')}.png?size=64`}
							alt=""
							width="36"
							height="36"
							loading="lazy"
							decoding="async"
						/>
						<span class="repo-name">
							<strong>{repository.fullName}</strong>
							<small>{lastPushLabel(repository.lastPushedAt)}</small>
						</span>
						<span class="activity">
							<GitCommitHorizontal aria-hidden="true" size={15} />
							<strong>{repository.commitCount}</strong>
							<small>commits</small>
						</span>
						<span class="arrow" aria-hidden="true"><ArrowRight size={15} /></span>
					</a>
				</li>
			{/each}
		</ol>
	{:else}
		<div class="empty">
			<GitCommitHorizontal aria-hidden="true" size={20} />
			<p>No public push events appeared in this recent window.</p>
		</div>
	{/if}

	<footer>
		<span>{insight.sourceDescription}</span>
		<span>{insight.cacheStatus.toLowerCase().replace('_', ' ')}</span>
	</footer>
</section>

<style>
	.commit-list {
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

	.window {
		display: inline-flex;
		min-height: 2rem;
		align-items: center;
		gap: var(--space-1);
		border-radius: var(--radius-pill);
		background: var(--info-muted);
		padding-inline: var(--space-3);
		color: var(--info-muted-foreground);
		font-size: var(--type-size-xs);
		font-weight: 650;
		white-space: nowrap;
	}

	ol {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	li + li {
		border-top: 1px solid var(--border);
	}

	li a {
		display: grid;
		min-height: 4.25rem;
		grid-template-columns: 1.5rem 2.25rem minmax(0, 1fr) auto 1.5rem;
		gap: var(--space-3);
		align-items: center;
		padding: var(--space-2) var(--space-5);
		color: var(--foreground);
		text-decoration: none;
		transition: background-color var(--motion-duration-fast) var(--motion-ease-standard);
	}

	li a:hover {
		background: var(--muted);
	}

	.rank {
		font-family: var(--font-mono);
		font-size: var(--type-size-xs);
		color: var(--muted-foreground);
	}

	li img {
		width: 2.25rem;
		height: 2.25rem;
		border-radius: var(--radius-md);
		background: var(--surface-inset);
		object-fit: cover;
	}

	.repo-name {
		min-width: 0;
	}

	.repo-name strong,
	.repo-name small {
		display: block;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.repo-name strong {
		font-size: var(--type-size-sm);
	}

	.repo-name small,
	.activity small {
		margin-top: 0.16rem;
		color: var(--muted-foreground);
		font-size: 0.68rem;
	}

	.activity {
		display: grid;
		grid-template-columns: auto auto;
		gap: 0 var(--space-1);
		align-items: center;
		font-family: var(--font-mono);
		font-size: var(--type-size-xs);
		text-align: right;
	}

	.activity small {
		grid-column: 1 / -1;
	}

	.arrow {
		color: var(--muted-foreground);
	}

	.empty {
		display: grid;
		place-items: center;
		gap: var(--space-2);
		padding: var(--space-8);
		color: var(--muted-foreground);
		font-size: var(--type-size-sm);
		text-align: center;
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

		li a {
			grid-template-columns: 2.25rem minmax(0, 1fr) auto;
			padding-inline: var(--space-4);
		}

		.rank,
		.activity small,
		.arrow {
			display: none;
		}
	}
</style>
