<script lang="ts">
  let { title, body }: { title: string; body: string } = $props();
</script>

<div class="rounded-[var(--radius-lg)] border border-[var(--destructive)] bg-[var(--destructive-muted)] p-6 text-[var(--destructive-muted-foreground)]" role="alert">
  <h2 class="font-[var(--font-display)] text-lg">{title}</h2>
  <p class="mt-2 text-sm">{body}</p>
</div>
