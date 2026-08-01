<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  let { class: className = '', children }: { class?: string; children?: Snippet } = $props();
</script>

<span class={cn('inline-flex items-center rounded-full bg-[var(--muted)] px-3 py-1 text-xs font-medium text-[var(--muted-foreground)]', className)}>
  {@render children?.()}
</span>
