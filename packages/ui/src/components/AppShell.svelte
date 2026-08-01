<script lang="ts">
  import type { Snippet } from 'svelte';
  import { page } from '$app/state';
  import {
    Bookmark,
    Compass,
    FolderKanban,
    GitBranch,
    History,
    Home,
    Menu,
    RefreshCcw
  } from 'lucide-svelte';
  import type { ConnectionStatus, SyncStatus } from '@gitexplore/api-client';
  import Badge from './Badge.svelte';
  import { cn } from '../lib/cn';

  let {
    authStatus,
    syncStatus,
    class: className = '',
    headerStatus,
    accountActions,
    children
  }: {
    authStatus: ConnectionStatus;
    syncStatus?: SyncStatus | null;
    class?: string;
    headerStatus?: Snippet;
    accountActions?: Snippet;
    children?: Snippet;
  } = $props();

  const primaryLinks = [
    { href: '/app/explore', label: 'Explore', icon: Compass },
    { href: '/app/bookmarks', label: 'Saved', icon: Bookmark },
    { href: '/app/explore/snapshots', label: 'Trails', icon: History }
  ];

  const utilityLinks = [
    { href: '/app', label: 'Overview', icon: Home },
    { href: '/app/categories', label: 'Collections', icon: FolderKanban },
    { href: '/app/sync', label: 'Sync & cache', icon: RefreshCcw }
  ];

  const links = [...primaryLinks, ...utilityLinks];

  function activeLink(pathname: string) {
    return links
      .filter((link) => pathname === link.href || pathname.startsWith(`${link.href}/`))
      .sort((left, right) => right.href.length - left.href.length)[0];
  }

  function isActive(href: string) {
    return activeLink(page.url.pathname)?.href === href;
  }

  const currentSection = $derived(activeLink(page.url.pathname)?.label ?? 'Explore');
  const accountLabel = $derived(
    authStatus.account?.login ? `@${authStatus.account.login}` : 'GitHub explorer'
  );
</script>

<a
  href="#gitexplore-main"
  class="fixed left-[var(--space-3)] top-[var(--space-3)] z-[var(--layer-tooltip)] -translate-y-24 rounded-[var(--radius-md)] bg-[var(--inverse)] px-[var(--space-4)] py-[var(--space-2)] text-sm font-semibold text-[var(--inverse-foreground)] transition-transform duration-[var(--motion-duration-fast)] focus:translate-y-0 motion-reduce:transition-none"
>
  Skip to content
</a>

<div class={cn('min-h-screen bg-[var(--background)] text-[var(--foreground)]', className)}>
  <div class="grid min-h-screen grid-cols-[minmax(0,1fr)] lg:grid-cols-[248px_minmax(0,1fr)]">
    <aside
      class="atlas-rail hidden border-r border-[var(--border)] bg-[color-mix(in_srgb,var(--surface)_94%,transparent)] backdrop-blur lg:block"
    >
      <div class="sticky top-0 flex h-screen flex-col">
        <div class="border-b border-[var(--border)] px-[var(--space-5)] py-[var(--space-6)]">
          <a
            href="/app/explore"
            class="group flex min-h-[var(--control-hit-target)] items-center gap-[var(--space-3)] rounded-[var(--radius-md)] focus-visible:outline-offset-4"
          >
            <span
              class="relative grid size-11 shrink-0 place-items-center rounded-[var(--radius-pill)] border border-[var(--border-strong)] bg-[var(--surface-raised)] text-[var(--primary)] shadow-[var(--shadow-soft)]"
            >
              <GitBranch aria-hidden="true" size={19} />
              <span
                aria-hidden="true"
                class="absolute -right-0.5 -top-0.5 size-2.5 rounded-[var(--radius-pill)] border-2 border-[var(--surface)] bg-[var(--success)]"
              ></span>
            </span>
            <span class="min-w-0">
              <span class="block text-xs font-semibold uppercase tracking-[var(--type-tracking-caps)] text-[var(--muted-foreground)]">
                GitExplore
              </span>
              <span class="mt-0.5 block font-[var(--font-heading)] text-lg font-semibold tracking-[var(--type-tracking-heading)]">
                Expedition atlas
              </span>
            </span>
          </a>
        </div>

        <nav aria-label="Primary" class="flex-1 overflow-y-auto px-[var(--space-3)] py-[var(--space-4)]">
          <p class="px-[var(--space-3)] text-xs font-semibold uppercase tracking-[var(--type-tracking-caps)] text-[var(--muted-foreground)]">
            Discovery
          </p>
          <div class="mt-[var(--space-2)] space-y-[var(--space-1)]">
            {#each primaryLinks as link}
              <a
                href={link.href}
                aria-current={isActive(link.href) ? 'page' : undefined}
                class={cn(
                  'group relative flex min-h-[var(--control-hit-target)] items-center gap-[var(--space-3)] rounded-[var(--radius-md)] px-[var(--space-3)] text-sm font-medium transition-[background-color,color,transform] duration-[var(--motion-duration-fast)] motion-reduce:transition-none',
                  isActive(link.href)
                    ? 'bg-[var(--primary)] text-[var(--primary-foreground)] shadow-[var(--shadow-soft)]'
                    : 'text-[var(--muted-foreground)] hover:bg-[var(--muted)] hover:text-[var(--foreground)]'
                )}
              >
                <link.icon aria-hidden="true" size={17} />
                <span>{link.label}</span>
                {#if isActive(link.href)}
                  <span aria-hidden="true" class="ml-auto size-1.5 rounded-[var(--radius-pill)] bg-current"></span>
                {/if}
              </a>
            {/each}
          </div>

          <div class="my-[var(--space-4)] h-px bg-[var(--border)]"></div>
          <p class="px-[var(--space-3)] text-xs font-semibold uppercase tracking-[var(--type-tracking-caps)] text-[var(--muted-foreground)]">
            Workspace
          </p>
          <div class="mt-[var(--space-2)] space-y-[var(--space-1)]">
            {#each utilityLinks as link}
              <a
                href={link.href}
                aria-current={isActive(link.href) ? 'page' : undefined}
                class={cn(
                  'flex min-h-[var(--control-hit-target)] items-center gap-[var(--space-3)] rounded-[var(--radius-md)] px-[var(--space-3)] text-sm font-medium transition-colors duration-[var(--motion-duration-fast)] motion-reduce:transition-none',
                  isActive(link.href)
                    ? 'bg-[var(--accent)] text-[var(--accent-foreground)] shadow-[inset_3px_0_0_var(--primary)]'
                    : 'text-[var(--muted-foreground)] hover:bg-[var(--muted)] hover:text-[var(--foreground)]'
                )}
              >
                <link.icon aria-hidden="true" size={17} />
                <span>{link.label}</span>
              </a>
            {/each}
          </div>
        </nav>

        <div class="border-t border-[var(--border)] p-[var(--space-4)]">
          <div class="rounded-[var(--radius-lg)] bg-[var(--surface-inset)] p-[var(--space-3)]">
            <div class="flex items-center gap-[var(--space-2)]">
              <span
                aria-hidden="true"
                class={cn(
                  'size-2 rounded-[var(--radius-pill)]',
                  authStatus.connected ? 'bg-[var(--success)]' : 'bg-[var(--warning)]'
                )}
              ></span>
              <p class="min-w-0 truncate text-sm font-semibold">{accountLabel}</p>
            </div>
            <p class="mt-[var(--space-1)] text-xs text-[var(--muted-foreground)]">
              {authStatus.connected ? 'GitHub connected' : 'Connection unavailable'}
            </p>
          </div>
        </div>
      </div>
    </aside>

    <div class="flex min-h-screen min-w-0 flex-col">
      <header
        class="sticky top-0 z-[var(--layer-header)] min-w-0 border-b border-[var(--border)] bg-[color-mix(in_srgb,var(--background)_90%,transparent)] backdrop-blur"
      >
        <div class="mx-auto flex min-h-16 w-full max-w-[var(--size-container-wide)] items-center justify-between gap-[var(--space-3)] px-[var(--space-4)] sm:px-[var(--space-6)] lg:px-[var(--space-8)]">
          <div class="flex min-w-0 items-center gap-[var(--space-3)]">
            <a
              href="/app/explore"
              aria-label="GitExplore expedition atlas"
              class="grid size-11 shrink-0 place-items-center rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--surface)] text-[var(--primary)] shadow-[var(--shadow-hairline)] lg:hidden"
            >
              <GitBranch aria-hidden="true" size={18} />
            </a>
            <div class="min-w-0">
              <p class="truncate text-xs font-semibold uppercase tracking-[var(--type-tracking-caps)] text-[var(--muted-foreground)]">
                {currentSection}
              </p>
              <p class="mt-0.5 truncate text-sm font-semibold sm:text-base">{accountLabel}</p>
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-[var(--space-2)]">
            {@render headerStatus?.()}
            {#if syncStatus}
              <span class="hidden sm:inline-flex"><Badge>{syncStatus.state}</Badge></span>
            {/if}
            <span
              class="hidden min-h-[var(--control-hit-target)] items-center gap-[var(--space-2)] rounded-[var(--radius-md)] bg-[var(--muted)] px-[var(--space-3)] text-sm font-medium text-[var(--foreground)] sm:inline-flex"
            >
              <span
                aria-hidden="true"
                class={cn(
                  'size-2 rounded-[var(--radius-pill)]',
                  authStatus.connected ? 'bg-[var(--success)]' : 'bg-[var(--warning)]'
                )}
              ></span>
              {authStatus.connected ? 'Connected' : 'Disconnected'}
            </span>

            <details class="relative lg:hidden">
              <summary
                aria-label="Open workspace navigation"
                class="grid min-h-[var(--control-hit-target)] min-w-[var(--control-hit-target)] cursor-pointer list-none place-items-center rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--surface)] text-[var(--muted-foreground)] marker:content-none"
              >
                <Menu aria-hidden="true" size={18} />
              </summary>
              <nav
                aria-label="Workspace"
                class="absolute right-0 top-[calc(100%+var(--space-2))] z-[var(--layer-dropdown)] w-56 rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--popover)] p-[var(--space-2)] text-[var(--popover-foreground)] shadow-[var(--shadow-elevated)]"
              >
                {#each utilityLinks as link}
                  <a
                    href={link.href}
                    aria-current={isActive(link.href) ? 'page' : undefined}
                    class={cn(
                      'flex min-h-[var(--control-hit-target)] items-center gap-[var(--space-3)] rounded-[var(--radius-md)] px-[var(--space-3)] text-sm font-medium',
                      isActive(link.href)
                        ? 'bg-[var(--accent)] text-[var(--accent-foreground)] shadow-[inset_3px_0_0_var(--primary)]'
                        : 'text-[var(--muted-foreground)] hover:bg-[var(--muted)] hover:text-[var(--foreground)]'
                    )}
                  >
                    <link.icon aria-hidden="true" size={17} />
                    {link.label}
                  </a>
                {/each}
              </nav>
            </details>
            {@render accountActions?.()}
          </div>
        </div>
      </header>

      <main id="gitexplore-main" tabindex="-1" class="min-w-0 flex-1 px-[var(--space-4)] py-[var(--space-6)] pb-28 sm:px-[var(--space-6)] lg:px-[var(--space-8)] lg:pb-[var(--space-8)]">
        <div class="mx-auto w-full max-w-[var(--size-container-wide)]">
          {@render children?.()}
        </div>
      </main>

      <nav
        aria-label="Primary"
        class="fixed inset-x-0 bottom-0 z-[var(--layer-header)] border-t border-[var(--border)] bg-[color-mix(in_srgb,var(--surface)_94%,transparent)] px-[var(--space-2)] pb-[max(var(--space-2),env(safe-area-inset-bottom))] pt-[var(--space-2)] backdrop-blur lg:hidden"
      >
        <div class="mx-auto grid max-w-md grid-cols-3 gap-[var(--space-1)]">
          {#each primaryLinks as link}
            <a
              href={link.href}
              aria-current={isActive(link.href) ? 'page' : undefined}
              class={cn(
                'flex min-h-[var(--control-hit-target)] flex-col items-center justify-center gap-1 rounded-[var(--radius-md)] px-[var(--space-2)] py-[var(--space-1)] text-xs font-medium transition-colors duration-[var(--motion-duration-fast)] motion-reduce:transition-none',
                isActive(link.href)
                  ? 'bg-[var(--accent)] text-[var(--accent-foreground)] shadow-[inset_0_0_0_1px_var(--primary)]'
                  : 'text-[var(--muted-foreground)] hover:bg-[var(--muted)] hover:text-[var(--foreground)]'
              )}
            >
              <link.icon aria-hidden="true" size={18} />
              <span>{link.label}</span>
            </a>
          {/each}
        </div>
      </nav>
    </div>
  </div>
</div>

<style>
  .atlas-rail {
    background-image:
      radial-gradient(circle at 22px 92px, color-mix(in srgb, var(--primary) 24%, transparent) 0 2px, transparent 2.5px),
      linear-gradient(90deg, transparent 21px, color-mix(in srgb, var(--border) 52%, transparent) 22px, transparent 23px);
    background-size: 100% 100%, 100% 100%;
  }

  summary::-webkit-details-marker {
    display: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .atlas-rail {
      background-image: none;
    }
  }
</style>
