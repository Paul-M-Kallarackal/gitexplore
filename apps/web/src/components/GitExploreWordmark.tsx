type GitExploreWordmarkProps = {
  className?: string;
};

export function GitExploreWordmark({ className }: GitExploreWordmarkProps) {
  return (
    <img
      alt=""
      aria-hidden="true"
      className={['brand-wordmark', className].filter(Boolean).join(' ')}
      decoding="async"
      draggable="false"
      height="295"
      src="/images/gitexplore-wordmark.png"
      width="1200"
    />
  );
}
