<script lang="ts">
  import type { Bookmark } from '@gitexplore/api-client';
  import CategoryPillGroup from './CategoryPillGroup.svelte';

  let { bookmark, selected = false }: { bookmark: Bookmark; selected?: boolean } = $props();

  function targetLabel(bookmark: Bookmark) {
    if ('GitHubUser' in bookmark.target) return bookmark.target.GitHubUser.login;
    return bookmark.target.GitHubRepository.full_name;
  }

  function avatarLogin(bookmark: Bookmark) {
    if ('GitHubUser' in bookmark.target) return bookmark.target.GitHubUser.login;
    return bookmark.target.GitHubRepository.full_name.split('/')[0] ?? '';
  }

  function targetKind(bookmark: Bookmark) {
    return 'GitHubUser' in bookmark.target ? 'Person' : 'Repository';
  }
</script>

<span class={`block rounded-[var(--radius-lg)] border bg-[var(--card)] p-4 shadow-[var(--shadow-soft)] transition-[border-color,background-color] duration-[var(--motion-duration-fast)] ${selected ? 'border-[var(--primary)] bg-[var(--accent)]/30' : 'border-[var(--border)]'}`}>
  <span class="flex items-start gap-3">
    <img
      src={`https://github.com/${encodeURIComponent(avatarLogin(bookmark))}.png?size=80`}
      alt=""
      width="40"
      height="40"
      loading="lazy"
      decoding="async"
      class="size-10 shrink-0 rounded-[var(--radius-md)] bg-[var(--surface-inset)] object-cover"
    />
    <span class="min-w-0 flex-1">
      <span class="flex items-start justify-between gap-3">
        <span class="min-w-0">
          <span class="block text-xs font-semibold uppercase tracking-[var(--type-tracking-caps)] text-[var(--muted-foreground)]">{targetKind(bookmark)}</span>
          <span class="mt-1 block truncate font-semibold">{targetLabel(bookmark)}</span>
        </span>
        <span class="shrink-0 text-xs text-[var(--muted-foreground)]">{new Date(bookmark.created_at).toLocaleDateString()}</span>
      </span>
      <span class="mt-2 block line-clamp-2 text-sm text-[var(--muted-foreground)]">{bookmark.note ?? 'Saved without a field note.'}</span>
    </span>
  </span>
  {#if bookmark.categories.length}
    <span class="mt-3 block">
      <CategoryPillGroup categories={bookmark.categories} />
    </span>
  {/if}
</span>
