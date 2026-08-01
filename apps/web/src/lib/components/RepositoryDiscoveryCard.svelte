<script lang="ts">
	import type { RepositoryCandidate } from '@gitexplore/api-client';
	import {
		ArrowRight,
		Bookmark,
		Check,
		Clock3,
		Code2,
		ExternalLink,
		GitFork,
		Sparkles,
		Star
	} from 'lucide-svelte';

	let {
		candidate,
		saving = false,
		saveError = null,
		onSave
	}: {
		candidate: RepositoryCandidate;
		saving?: boolean;
		saveError?: string | null;
		onSave: (candidate: RepositoryCandidate) => void | Promise<void>;
	} = $props();

	const repository = $derived(candidate.repository);
	const numberFormat = new Intl.NumberFormat(undefined, { notation: 'compact' });
	const detailHref = $derived(
		`/app/repository/${encodeURIComponent(repository.ownerLogin)}/${encodeURIComponent(repository.name)}`
	);
	const ownerAvatar = $derived(`https://github.com/${encodeURIComponent(repository.ownerLogin)}.png?size=96`);

	function formatNumber(value: number) {
		return numberFormat.format(value);
	}

	function updatedLabel(value: string | null) {
		if (!value) return 'Activity unknown';

		const timestamp = new Date(value).getTime();
		if (!Number.isFinite(timestamp)) return 'Activity unknown';

		const days = Math.max(0, Math.floor((Date.now() - timestamp) / 86_400_000));
		if (days === 0) return 'Today';
		if (days === 1) return 'Yesterday';
		if (days < 30) return `${days}d ago`;
		if (days < 365) return `${Math.floor(days / 30)}mo ago`;
		return `${Math.floor(days / 365)}y ago`;
	}
</script>

<article class="repository-card">
	<header class="card-header">
		<img
			src={ownerAvatar}
			alt=""
			width="48"
			height="48"
			loading="lazy"
			decoding="async"
			class="owner-avatar"
		/>
		<div class="identity">
			<p>{repository.ownerLogin}</p>
			<h3><a href={detailHref} aria-label={`Inspect ${repository.fullName}`}>{repository.name}</a></h3>
		</div>
		<button
			type="button"
			disabled={candidate.saved || saving}
			onclick={() => onSave(candidate)}
			aria-label={candidate.saved ? `${repository.fullName} is saved` : `Save ${repository.fullName}`}
			class:already-saved={candidate.saved}
			class="save-button"
		>
			{#if candidate.saved}
				<Check aria-hidden="true" size={16} />
				<span>Saved</span>
			{:else}
				<Bookmark aria-hidden="true" size={16} />
				<span>{saving ? 'Saving…' : 'Save'}</span>
			{/if}
		</button>
	</header>

	<p class="description">
		{repository.description || 'No description supplied by the repository owner.'}
	</p>

	<div class="reason">
		<Sparkles aria-hidden="true" size={15} />
		<p>{candidate.reasons[0] || 'Strong activity close to people in this neighborhood.'}</p>
	</div>

	<div class="metrics" role="list" aria-label="Repository details">
		<span role="listitem" title={`${repository.stargazerCount} GitHub stars`}>
			<Star aria-hidden="true" size={14} /><span class="sr-only">GitHub stars:</span> {formatNumber(repository.stargazerCount)}
		</span>
		<span role="listitem" title={`${repository.forkCount} forks`}>
			<GitFork aria-hidden="true" size={14} /><span class="sr-only">Forks:</span> {formatNumber(repository.forkCount)}
		</span>
		{#if repository.primaryLanguage}
			<span role="listitem"><Code2 aria-hidden="true" size={14} /><span class="sr-only">Primary language:</span> {repository.primaryLanguage}</span>
		{/if}
		<span role="listitem"><Clock3 aria-hidden="true" size={14} /><span class="sr-only">Updated:</span> {updatedLabel(repository.updatedAt)}</span>
	</div>

	<footer>
		<div class="via" role="group" aria-label={`Surfaced through ${candidate.viaLogins.length} nearby people`}>
			{#if candidate.viaLogins.length}
				<div class="avatar-stack" aria-hidden="true">
					{#each candidate.viaLogins.slice(0, 3) as login}
						<img
							src={`https://github.com/${encodeURIComponent(login)}.png?size=48`}
							alt=""
							width="24"
							height="24"
							loading="lazy"
							decoding="async"
						/>
					{/each}
				</div>
				<span>{candidate.networkStars} nearby signal{candidate.networkStars === 1 ? '' : 's'}</span>
			{:else}
				<span>{Math.round(candidate.discoveryScore)} discovery signal</span>
			{/if}
		</div>
		<div class="card-links">
			<a href={repository.htmlUrl} target="_blank" rel="noreferrer" aria-label={`Open ${repository.fullName} on GitHub in a new tab`}>
				<ExternalLink aria-hidden="true" size={16} />
			</a>
			<a href={detailHref} class="inspect-link" aria-label={`Inspect ${repository.fullName}`}>
				Inspect <ArrowRight aria-hidden="true" size={15} />
			</a>
		</div>
	</footer>

	{#if saveError}
		<p class="save-error" role="alert">{saveError}</p>
	{/if}
</article>

<style>
	.repository-card {
		display: flex;
		height: 100%;
		min-width: 0;
		flex-direction: column;
		gap: var(--space-4);
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: var(--card);
		padding: var(--space-5);
		box-shadow: var(--shadow-soft);
		transition: border-color var(--motion-duration-fast) var(--motion-ease-standard), box-shadow var(--motion-duration-fast) var(--motion-ease-standard), transform var(--motion-duration-fast) var(--motion-ease-standard);
	}

	.repository-card:hover {
		border-color: var(--border-strong);
		box-shadow: var(--shadow-card);
		transform: translateY(-2px);
	}

	.card-header {
		display: grid;
		grid-template-columns: 3rem minmax(0, 1fr) auto;
		gap: var(--space-3);
		align-items: center;
	}

	.owner-avatar {
		width: 3rem;
		height: 3rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: var(--surface-inset);
		object-fit: cover;
	}

	.identity {
		min-width: 0;
	}

	.identity p {
		margin: 0;
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
	}

	.identity h3 {
		margin: var(--space-1) 0 0;
		font-size: var(--type-size-lg);
		font-weight: 680;
		letter-spacing: -0.025em;
	}

	.identity h3 a {
		display: block;
		overflow: hidden;
		color: var(--foreground);
		text-decoration: none;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.identity h3 a::after {
		position: absolute;
		inset: 0;
		content: '';
	}

	.repository-card {
		position: relative;
	}

	.save-button,
	.card-links a {
		position: relative;
		z-index: 1;
	}

	.save-button {
		display: inline-flex;
		min-height: var(--control-hit-target);
		align-items: center;
		justify-content: center;
		gap: var(--space-2);
		border: 0;
		border-radius: var(--radius-md);
		background: var(--primary);
		padding-inline: var(--space-3);
		color: var(--primary-foreground);
		font: inherit;
		font-size: var(--type-size-sm);
		font-weight: 700;
		cursor: pointer;
	}

	.save-button.already-saved {
		background: var(--success-muted);
		color: var(--success-muted-foreground);
	}

	.save-button:disabled {
		cursor: default;
		opacity: var(--effect-disabled-opacity);
	}

	.save-button.already-saved:disabled {
		opacity: 1;
	}

	.description {
		display: -webkit-box;
		overflow: hidden;
		min-height: 3rem;
		margin: 0;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		color: var(--muted-foreground);
		font-size: var(--type-size-sm);
		line-height: 1.55;
	}

	.reason {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr);
		gap: var(--space-2);
		align-items: start;
		border-radius: var(--radius-md);
		background: var(--surface-inset);
		padding: var(--space-3);
		color: var(--accent-foreground);
	}

	.reason :global(svg) {
		margin-top: 0.15rem;
		color: var(--primary);
	}

	.reason p {
		display: -webkit-box;
		overflow: hidden;
		margin: 0;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		font-size: var(--type-size-xs);
		line-height: 1.5;
	}

	.metrics {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2) var(--space-4);
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
	}

	.metrics span {
		display: inline-flex;
		align-items: center;
		gap: var(--space-1);
	}

	footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		margin-top: auto;
		padding-top: var(--space-3);
		border-top: 1px solid var(--border);
	}

	.via {
		display: flex;
		min-width: 0;
		align-items: center;
		gap: var(--space-2);
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
	}

	.via > span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.avatar-stack {
		display: flex;
		flex: 0 0 auto;
		padding-left: 0.35rem;
	}

	.avatar-stack img {
		width: 1.5rem;
		height: 1.5rem;
		margin-left: -0.35rem;
		border: 2px solid var(--card);
		border-radius: var(--radius-pill);
		background: var(--surface-inset);
		object-fit: cover;
	}

	.card-links {
		display: flex;
		flex: 0 0 auto;
		align-items: center;
		gap: var(--space-1);
	}

	.card-links a {
		display: inline-flex;
		min-width: var(--control-hit-target);
		min-height: var(--control-hit-target);
		align-items: center;
		justify-content: center;
		gap: var(--space-1);
		border-radius: var(--radius-md);
		color: var(--muted-foreground);
		font-size: var(--type-size-sm);
		font-weight: 700;
		text-decoration: none;
	}

	.card-links a:hover {
		background: var(--muted);
		color: var(--foreground);
	}

	.card-links .inspect-link {
		padding-inline: var(--space-2);
		color: var(--primary);
	}

	.save-error {
		position: relative;
		z-index: 1;
		margin: 0;
		color: var(--destructive);
		font-size: var(--type-size-sm);
	}

	@media (max-width: 28rem) {
		.save-button span,
		.via > span {
			display: none;
		}

		.save-button {
			width: var(--control-hit-target);
			padding: 0;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.repository-card:hover {
			transform: none;
		}
	}
</style>
