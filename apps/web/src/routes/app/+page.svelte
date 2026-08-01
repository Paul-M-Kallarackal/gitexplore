<svelte:head>
	<title>Opening your graph · GitExplore</title>
</svelte:head>

<div class="opening" role="status" aria-live="polite">
	<span aria-hidden="true"></span>
	<h1>Opening your GitHub expedition…</h1>
</div>

<style>
	.opening {
		display: grid;
		min-height: 50svh;
		place-items: center;
		align-content: center;
		gap: var(--space-3);
		color: var(--muted-foreground);
		font-size: var(--type-size-sm);
	}

	.opening span {
		width: 2.5rem;
		height: 2.5rem;
		border: 2px solid var(--border);
		border-top-color: var(--primary);
		border-radius: var(--radius-pill);
		animation: spin 700ms linear infinite;
	}

	@keyframes spin {
		to { transform: rotate(360deg); }
	}

	@media (prefers-reduced-motion: reduce) {
		.opening span { animation: none; }
	}
</style>
