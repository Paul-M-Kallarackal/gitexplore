import type {
	Bookmark,
	BookmarkTarget,
	Category,
	ConnectedAccount,
	ExplorationSeed,
	ExplorationSnapshot,
	SyncStatus
} from '@gitexplore/api-client';

export function formatTimestamp(value: string | null) {
	if (!value) {
		return 'Never';
	}

	return new Date(value).toLocaleString();
}

export function describeBookmarkTarget(target: BookmarkTarget) {
	if ('GitHubUser' in target) {
		return target.GitHubUser.login;
	}

	return target.GitHubRepository.full_name;
}

export function bookmarkKind(target: BookmarkTarget) {
	return 'GitHubUser' in target ? 'Person' : 'Repository';
}

export function accountLabel(account: ConnectedAccount | null) {
	return account?.display_name || account?.login || 'Disconnected';
}

export function syncTone(status: SyncStatus['state']) {
	switch (status) {
		case 'SyncSucceeded':
			return 'text-emerald-700';
		case 'SyncFailed':
			return 'text-orange-700';
		case 'SyncInProgress':
			return 'text-sky-700';
		default:
			return 'text-[var(--muted-foreground)]';
	}
}

export function seedLabel(seed: ExplorationSeed) {
	if ('User' in seed) {
		return seed.User.login;
	}

	if ('Repository' in seed) {
		return seed.Repository.full_name;
	}

	return seed.Category.name;
}

export function snapshotSummary(snapshot: ExplorationSnapshot) {
	return `${snapshot.discovered_people.length} people • ${snapshot.discovered_repositories.length} repositories`;
}

export function categorySummary(categories: Category[]) {
	return `${categories.length} ${categories.length === 1 ? 'category' : 'categories'}`;
}

export function recentBookmarks(bookmarks: Bookmark[], limit = 5) {
	return bookmarks.slice(0, limit);
}
