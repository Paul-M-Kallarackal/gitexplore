<script lang="ts">
	import type { PageProps } from './$types';
	import { ArrowRight, Bookmark, Compass, GitBranch, ShieldCheck } from 'lucide-svelte';
	import { createGitExploreApi } from '@gitexplore/api-client';

	let { data }: PageProps = $props();

	const api = $derived(createGitExploreApi({ baseUrl: data.apiBaseUrl }));
	const connectUrl = $derived(api.startBrowserOAuth(`${data.appOrigin}/app`));
</script>

<svelte:head>
	<title>Sign in · GitExplore</title>
	<meta
		name="description"
		content="Explore the people and repositories around your GitHub network, then save the uncommon finds."
	/>
</svelte:head>

<main class="login-page">
	<header class="login-bar">
		<a class="brand" href="/" aria-label="GitExplore home">
			<span class="brand-mark" aria-hidden="true"><GitBranch size={18} /></span>
			<span>GitExplore</span>
		</a>
		<p>Shared graph <span aria-hidden="true">·</span> private field notes</p>
	</header>

	<div class="login-grid">
		<section class="atlas" aria-labelledby="login-title">
			<img
				src="/images/gitexplore-atlas.webp"
				alt=""
				width="1440"
				height="960"
				fetchpriority="high"
				decoding="async"
			/>
			<div class="atlas-copy">
				<p class="atlas-kicker">Your open-source expedition</p>
				<h1 id="login-title">Follow the signal.<br />Keep the rare finds.</h1>
				<p>
					Move through followers, collaborators, and repositories while GitExplore remembers
					the trail for you.
				</p>
			</div>
			<ol class="route-key" aria-label="How exploration works" role="list">
				<li><Compass aria-hidden="true" size={15} /> Pick a person</li>
				<li><GitBranch aria-hidden="true" size={15} /> Follow a branch</li>
				<li><Bookmark aria-hidden="true" size={15} /> Save the find</li>
			</ol>
		</section>

		<section class="connect-card" aria-labelledby="connect-title">
			<div>
				<p class="eyebrow">Start exploring</p>
				<h2 id="connect-title">Connect your GitHub account</h2>
				<p class="connect-copy">
					Sign in once to begin from your own network. Public graph facts are cached and
					shared; your bookmarks stay private to you.
				</p>
			</div>

			<ul class="trust-list" role="list">
				<li>
					<span aria-hidden="true"><ShieldCheck size={17} /></span>
					<div>
						<strong>Session cookie, not a user ID</strong>
						<p>The Rust service owns your authenticated browser session.</p>
					</div>
				</li>
				<li>
					<span aria-hidden="true"><GitBranch size={17} /></span>
					<div>
						<strong>Rate-aware by design</strong>
						<p>Cached nodes remain useful while fresh GitHub data is collected.</p>
					</div>
				</li>
			</ul>

			<div class="connect-action">
				<a href={connectUrl} class="connect-button">
					<span>
						<GitBranch aria-hidden="true" size={17} />
						Continue with GitHub
					</span>
					<ArrowRight aria-hidden="true" size={17} />
				</a>
				<p>Uses read-only profile access. You can sign out at any time.</p>
			</div>
		</section>
	</div>
</main>

<style>
	.login-page {
		width: min(100% - 2rem, 78rem);
		min-height: 100svh;
		margin-inline: auto;
		padding-block: var(--space-4) var(--space-8);
	}

	.login-bar {
		display: flex;
		min-height: 4rem;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-4);
	}

	.login-bar p {
		margin: 0;
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
		letter-spacing: var(--type-tracking-caps);
		text-transform: uppercase;
	}

	.brand {
		display: inline-flex;
		min-height: var(--control-hit-target);
		align-items: center;
		gap: var(--space-2);
		color: var(--foreground);
		font-weight: 700;
		letter-spacing: -0.02em;
		text-decoration: none;
	}

	.brand-mark {
		display: grid;
		width: 2rem;
		height: 2rem;
		place-items: center;
		border-radius: var(--radius-sm);
		background: var(--primary);
		color: var(--primary-foreground);
	}

	.login-grid {
		display: grid;
		gap: var(--space-4);
	}

	.atlas,
	.connect-card {
		position: relative;
		overflow: hidden;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		box-shadow: var(--shadow-card);
	}

	.atlas {
		min-height: 34rem;
		background: #efe6d2;
	}

	.atlas::after {
		position: absolute;
		inset: 0;
		background: linear-gradient(90deg, rgb(248 240 221 / 0.97) 0%, rgb(248 240 221 / 0.87) 31%, transparent 62%);
		content: '';
		pointer-events: none;
	}

	.atlas > img {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: cover;
	}

	.atlas-copy {
		position: relative;
		z-index: 1;
		width: min(90%, 31rem);
		padding: clamp(2rem, 6vw, 4.5rem);
		color: #2f2237;
	}

	.atlas-kicker {
		margin: 0;
		color: #765092;
		font-size: var(--type-size-xs);
		font-weight: 700;
		letter-spacing: var(--type-tracking-caps);
		text-transform: uppercase;
	}

	.atlas h1 {
		max-width: 11ch;
		margin: var(--space-4) 0 0;
		font-size: clamp(2.5rem, 5vw, 4.8rem);
		font-weight: 650;
		letter-spacing: -0.055em;
		line-height: 0.98;
	}

	.atlas-copy > p:last-child {
		max-width: 32rem;
		margin: var(--space-5) 0 0;
		color: #5f5166;
		font-size: var(--type-size-sm);
		line-height: 1.65;
	}

	.route-key {
		position: absolute;
		z-index: 2;
		right: var(--space-4);
		bottom: var(--space-4);
		left: var(--space-4);
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.route-key li {
		display: inline-flex;
		min-height: 2.25rem;
		align-items: center;
		gap: var(--space-2);
		border: 1px solid rgb(76 49 94 / 0.14);
		border-radius: var(--radius-pill);
		background: rgb(255 252 245 / 0.9);
		padding-inline: var(--space-3);
		color: #4d3262;
		font-size: var(--type-size-xs);
		font-weight: 650;
		backdrop-filter: blur(8px);
	}

	.connect-card {
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		gap: var(--space-8);
		background: color-mix(in srgb, var(--card) 96%, transparent);
		padding: clamp(1.5rem, 4vw, 3rem);
	}

	.connect-card h2 {
		max-width: 13ch;
		margin: var(--space-3) 0 0;
		font-size: clamp(2rem, 4vw, 3rem);
		font-weight: 650;
		letter-spacing: var(--type-tracking-tight);
		line-height: 1.02;
	}

	.connect-copy {
		max-width: 34rem;
		margin: var(--space-4) 0 0;
		color: var(--muted-foreground);
		font-size: var(--type-size-sm);
		line-height: 1.65;
	}

	.trust-list {
		display: grid;
		gap: var(--space-3);
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.trust-list li {
		display: grid;
		grid-template-columns: 2.25rem minmax(0, 1fr);
		gap: var(--space-3);
		align-items: start;
	}

	.trust-list li > span {
		display: grid;
		width: 2.25rem;
		height: 2.25rem;
		place-items: center;
		border-radius: var(--radius-md);
		background: var(--accent);
		color: var(--accent-foreground);
	}

	.trust-list strong {
		display: block;
		font-size: var(--type-size-sm);
	}

	.trust-list p {
		margin: var(--space-1) 0 0;
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
		line-height: 1.5;
	}

	.connect-action {
		padding-top: var(--space-5);
		border-top: 1px solid var(--border);
	}

	.connect-button {
		display: flex;
		min-height: 3.25rem;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		border-radius: var(--radius-md);
		background: var(--primary);
		padding-inline: var(--space-4);
		color: var(--primary-foreground);
		font-size: var(--type-size-sm);
		font-weight: 700;
		text-decoration: none;
		transition: background-color var(--motion-duration-fast) var(--motion-ease-standard), transform var(--motion-duration-fast) var(--motion-ease-standard);
	}

	.connect-button span {
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
	}

	.connect-button:hover {
		background: color-mix(in srgb, var(--primary) 90%, var(--foreground));
		transform: translateY(-1px);
	}

	.connect-action p {
		margin: var(--space-3) 0 0;
		color: var(--muted-foreground);
		font-size: var(--type-size-xs);
	}

	@media (min-width: 62rem) {
		.login-page {
			padding-block: var(--space-5);
		}

		.login-grid {
			grid-template-columns: minmax(0, 1.55fr) minmax(20rem, 0.8fr);
			min-height: calc(100svh - 7rem);
		}
	}

	@media (max-width: 40rem) {
		.login-bar p {
			display: none;
		}

		.atlas {
			min-height: 30rem;
		}

		.atlas::after {
			background: linear-gradient(180deg, rgb(248 240 221 / 0.98) 0%, rgb(248 240 221 / 0.72) 58%, transparent 100%);
		}

		.atlas-copy {
			width: auto;
			padding: var(--space-6);
		}

		.atlas h1 {
			font-size: clamp(2.4rem, 12vw, 3.5rem);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.connect-button:hover {
			transform: none;
		}
	}
</style>
