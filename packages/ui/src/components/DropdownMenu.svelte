<details class="relative">
  <summary class="list-none cursor-pointer">
    <slot name="trigger" />
  </summary>
  <div class="absolute right-0 mt-2 min-w-40 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--popover)] p-2 text-[var(--popover-foreground)] shadow-[var(--shadow-elevated)]">
    <slot />
  </div>
</details>
