<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import { cn } from '../lib/cn';

  let {
    class: className = '',
    variant = 'primary',
    type = 'button',
    disabled = false,
    children,
    ...rest
  }: {
    class?: string;
    variant?: 'primary' | 'secondary' | 'ghost';
    type?: 'button' | 'submit' | 'reset';
    disabled?: boolean;
    children?: Snippet;
  } & HTMLButtonAttributes = $props();

  const variants = {
    primary: 'bg-[var(--primary)] text-[var(--primary-foreground)] hover:opacity-90',
    secondary: 'bg-[var(--muted)] text-[var(--foreground)] hover:bg-[var(--border)]',
    ghost: 'bg-transparent text-[var(--foreground)] hover:bg-[var(--muted)]'
  };
</script>

<button
  {type}
  {disabled}
  {...rest}
  class={cn(
    'inline-flex min-h-[var(--control-hit-target)] items-center justify-center gap-2 rounded-[var(--radius-md)] px-4 text-sm font-medium transition-[background-color,color,opacity,transform] duration-[var(--motion-duration-fast)] ease-[var(--motion-ease-standard)] active:scale-[var(--motion-scale-press)] disabled:cursor-not-allowed disabled:opacity-[var(--effect-disabled-opacity)] disabled:active:scale-100 motion-reduce:transform-none motion-reduce:transition-none',
    variants[variant],
    className
  )}
>
  {@render children?.()}
</button>
