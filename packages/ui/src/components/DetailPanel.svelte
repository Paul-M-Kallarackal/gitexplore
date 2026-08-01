<script lang="ts">
  import Card from './Card.svelte';
  let { title, subtitle = '', metadata = [] as Array<{ label: string; value: string }> } = $props();
</script>

<Card class="sticky top-24 p-5">
  <section aria-labelledby="detail-panel-title">
    <p class="text-xs uppercase tracking-[0.3em] text-[var(--muted-foreground)]">Detail</p>
    <h2 id="detail-panel-title" class="mt-2 font-[var(--font-display)] text-xl">{title}</h2>
    {#if subtitle}
      <p class="mt-2 text-sm text-[var(--muted-foreground)]">{subtitle}</p>
    {/if}
    <dl class="mt-5 space-y-3">
      {#each metadata as item}
        <div class="rounded-xl bg-[var(--muted)] p-3">
          <dt class="text-xs uppercase tracking-[0.2em] text-[var(--muted-foreground)]">{item.label}</dt>
          <dd class="mt-1 text-sm">{item.value}</dd>
        </div>
      {/each}
    </dl>
  </section>
</Card>
