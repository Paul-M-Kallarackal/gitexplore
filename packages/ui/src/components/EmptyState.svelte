<script lang="ts">
  let { title, body }: { title: string; body: string } = $props();
</script>

<div class="rounded-[var(--radius-lg)] border border-dashed border-[var(--border-strong)] bg-[var(--surface)] p-8 text-center">
  <h2 class="font-[var(--font-display)] text-xl">{title}</h2>
  <p class="mt-2 text-sm text-[var(--muted-foreground)]">{body}</p>
</div>
