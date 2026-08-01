<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '../lib/cn';

  let {
    class: className = '',
    type = 'text',
    value = $bindable(''),
    placeholder = '',
    name = '',
    id = '',
    required = false,
    ...rest
  }: {
    class?: string;
    type?: string;
    value?: string;
    placeholder?: string;
    name?: string;
    id?: string;
    required?: boolean;
  } & HTMLInputAttributes = $props();
</script>

<input
  {type}
  {value}
  {placeholder}
  {name}
  {id}
  {required}
  {...rest}
  class={cn(
    'min-h-[var(--control-hit-target)] w-full rounded-[var(--radius-md)] border border-[color-mix(in_srgb,var(--input)_65%,var(--foreground))] bg-[var(--surface)] px-3 text-sm text-[var(--foreground)] outline-none transition-[border-color,box-shadow] duration-[var(--motion-duration-fast)] focus:border-[var(--ring)] focus:shadow-[var(--shadow-focus)]',
    className
  )}
/>
