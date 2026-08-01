<script lang="ts">
  import Card from './Card.svelte';
  let {
    title,
    items = [] as string[]
  }: {
    title: string;
    items: string[];
  } = $props();
</script>

<Card class="p-5">
  <h3 class="font-[var(--font-display)] text-lg">{title}</h3>
  <div class="mt-4 space-y-2">
    {#if items.length}
      {#each items as item}
        <div class="rounded-xl bg-[var(--muted)] px-3 py-2 text-sm">{item}</div>
      {/each}
    {:else}
      <p class="text-sm text-[var(--muted-foreground)]">No items yet.</p>
    {/if}
  </div>
</Card>
