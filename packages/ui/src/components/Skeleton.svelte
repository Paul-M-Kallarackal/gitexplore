<script lang="ts">
  import { cn } from '../lib/cn';
  let { class: className = '' }: { class?: string } = $props();
</script>

<div class={cn('animate-pulse rounded-xl bg-[var(--muted)] motion-reduce:animate-none', className)} aria-hidden="true"></div>
