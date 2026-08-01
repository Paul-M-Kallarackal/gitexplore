<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  let {
    open = false,
    class: className = '',
    children
  }: {
    open?: boolean;
    class?: string;
    children?: Snippet;
  } = $props();
</script>

{#if open}
  <div class="fixed inset-0 z-40 bg-black/20">
    <aside class={cn('absolute right-0 top-0 h-full w-full max-w-md border-l border-[var(--border)] bg-[var(--surface-raised)] p-6 shadow-[var(--shadow-elevated)]', className)}>
      {@render children?.()}
    </aside>
  </div>
{/if}
