<script lang="ts">
  import type { Snippet } from 'svelte';
  let { text = '', children }: { text?: string; children?: Snippet } = $props();
</script>

<span title={text}>
  {@render children?.()}
</span>
