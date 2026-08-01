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
  <div class="fixed inset-0 z-50 grid place-items-center bg-black/20 p-4">
    <div class={cn('w-full max-w-lg rounded-[var(--radius-xl)] bg-[var(--surface-raised)] p-6 shadow-[var(--shadow-elevated)]', className)}>
      {@render children?.()}
    </div>
  </div>
{/if}
