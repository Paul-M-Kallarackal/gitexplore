<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  let { class: className = '', children }: { class?: string; children?: Snippet } = $props();
</script>

<div class={cn('rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--card)] shadow-[var(--shadow-soft)]', className)}>
  {@render children?.()}
</div>
