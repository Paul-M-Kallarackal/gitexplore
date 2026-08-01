import type { Bookmark, BookmarkTarget, ExplorationSeed, ExplorationSnapshot } from '@gitexplore/api-client';

export function formatTimestamp(value: string | null) {
  if (!value) return 'Not yet';
  return new Date(value).toLocaleString();
}

export function describeBookmarkTarget(target: BookmarkTarget) {
  return 'GitHubUser' in target ? target.GitHubUser.login : target.GitHubRepository.full_name;
}

export function bookmarkKind(target: BookmarkTarget) {
  return 'GitHubUser' in target ? 'Person' : 'Repository';
}

export function seedLabel(seed: ExplorationSeed) {
  if ('User' in seed) return seed.User.login;
  if ('Repository' in seed) return seed.Repository.full_name;
  return seed.Category.name;
}

export function snapshotSummary(snapshot: ExplorationSnapshot) {
  return `${snapshot.discovered_people.length} people · ${snapshot.discovered_repositories.length} repositories`;
}

export function recentBookmarks(bookmarks: Bookmark[], limit = 5) {
  return bookmarks.slice(0, limit);
}

export function compactNumber(value: number | null | undefined) {
  if (value == null) return '—';
  return new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(value);
}

export function cacheLabel(value: string) {
  return value.toLowerCase().replaceAll('_', ' ');
}
