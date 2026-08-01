<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import { LogOut } from 'lucide-svelte';
	import { AppShell, Button } from '@gitexplore/ui';
	import { createBrowserApi } from '$lib/api';
	import RateBudget from '$lib/components/RateBudget.svelte';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();
	let signingOut = $state(false);
	let logoutError = $state('');

	const api = $derived(createBrowserApi(data.apiBaseUrl));
	const rateLimitQuery = createQuery(() => ({
		queryKey: ['github-rate-limit'],
		queryFn: () => api.getRateLimit(),
		staleTime: 60_000,
		refetchInterval: 5 * 60_000,
		refetchOnWindowFocus: false,
		retry: 1
	}));

	async function signOut() {
		if (signingOut) return;
		signingOut = true;
		logoutError = '';
		try {
			await api.logout();
			window.location.assign('/login');
		} catch (error) {
			logoutError = error instanceof Error ? error.message : 'Sign out failed.';
			signingOut = false;
		}
	}
</script>

{#snippet accountActions()}
	<Button variant="ghost" disabled={signingOut} onclick={signOut}>
		<LogOut aria-hidden="true" size={16} />
		{signingOut ? 'Signing out…' : 'Sign out'}
	</Button>
	{#if logoutError}
		<span class="sr-only" role="alert">{logoutError}</span>
	{/if}
{/snippet}

{#snippet headerStatus()}
	<RateBudget rate={rateLimitQuery.data} pending={rateLimitQuery.isPending} compact />
{/snippet}

<AppShell authStatus={data.authStatus} syncStatus={data.syncStatus} {headerStatus} {accountActions}>
	{@render children()}
</AppShell>
