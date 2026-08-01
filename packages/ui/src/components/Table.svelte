<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  let { class: className = '', children }: { class?: string; children?: Snippet } = $props();
</script>

<div class={cn('overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface)]', className)}>
  <table class="w-full text-left text-sm">
    {@render children?.()}
  </table>
</div>
