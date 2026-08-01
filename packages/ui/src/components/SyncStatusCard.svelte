<script lang="ts">
  import type { SyncStatus, SyncSummary } from '@gitexplore/api-client';
  import Card from './Card.svelte';
  import Badge from './Badge.svelte';

  let {
    status,
    summary
  }: {
    status: SyncStatus;
    summary?: SyncSummary | null;
  } = $props();
</script>

<Card class="p-6">
  <div class="flex items-start justify-between gap-4">
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-[var(--muted-foreground)]">Sync status</p>
      <h2 class="mt-2 font-[var(--font-display)] text-2xl">{status.state}</h2>
      <p class="mt-2 text-sm text-[var(--muted-foreground)]">
        Last synced: {status.last_synced_at ?? 'never'}
      </p>
    </div>
    <Badge>{status.state}</Badge>
  </div>

  {#if summary}
    <div class="mt-6 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
      <div class="rounded-xl bg-[var(--muted)] p-4 text-sm">Followers: {summary.followers}</div>
      <div class="rounded-xl bg-[var(--muted)] p-4 text-sm">Following: {summary.following}</div>
      <div class="rounded-xl bg-[var(--muted)] p-4 text-sm">Starred: {summary.starred_repositories}</div>
      <div class="rounded-xl bg-[var(--muted)] p-4 text-sm">Repos: {summary.repositories}</div>
    </div>
  {/if}
</Card>
