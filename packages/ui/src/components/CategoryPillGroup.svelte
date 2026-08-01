<script lang="ts">
  import Badge from './Badge.svelte';
  let { categories = [] as string[] } = $props();
</script>

<span class="flex flex-wrap gap-2">
  {#each categories as category}
    <Badge>{category}</Badge>
  {/each}
</span>
