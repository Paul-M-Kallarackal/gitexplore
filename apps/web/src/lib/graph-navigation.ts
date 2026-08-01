const githubLoginPattern = /^[a-z\d](?:[a-z\d-]{0,37}[a-z\d])?$/i;
export const maxTrailEntries = 8;
export type ConnectionDirection = 'followers' | 'following';

export function normalizeLoginInput(value: string) {
	let login = value.trim();

	if (login.startsWith('@')) {
		login = login.slice(1);
	}

	login = login.replace(/^https?:\/\/(?:www\.)?github\.com\//i, '');
	login = login.split(/[/?#]/, 1)[0] ?? '';

	return login.trim();
}

export function isLikelyGitHubLogin(value: string) {
	return githubLoginPattern.test(normalizeLoginInput(value));
}

export function normalizeTrail(value: string | null, currentLogin: string) {
	const current = normalizeLoginInput(currentLogin);
	return appendTrail(parseTrail(value), current);
}

export function parseTrail(value: string | null) {
	return (value ?? '')
		.split(',')
		.map(normalizeLoginInput)
		.filter((login) => githubLoginPattern.test(login))
		.slice(-maxTrailEntries);
}

export function appendTrail(trail: string[], login: string) {
	const nextLogin = normalizeLoginInput(login);
	const nextTrail = trail.filter((item) => githubLoginPattern.test(item));

	if (!nextLogin) {
		return nextTrail.slice(-maxTrailEntries);
	}

	const existingIndex = nextTrail.findIndex(
		(item) => item.toLowerCase() === nextLogin.toLowerCase()
	);

	if (existingIndex >= 0) {
		return nextTrail.slice(0, existingIndex + 1).slice(-maxTrailEntries);
	}

	nextTrail.push(nextLogin);

	return nextTrail.slice(-maxTrailEntries);
}

export function normalizeConnectionDirection(value: string | null | undefined): ConnectionDirection {
	return value === 'following' ? 'following' : 'followers';
}

export function setConnectionDirection(
	searchParams: URLSearchParams,
	direction: string
) {
	const next = new URLSearchParams(searchParams);
	next.set('direction', normalizeConnectionDirection(direction));
	return next;
}

export function buildExploreHref(
	login: string,
	trail: string[] = [],
	direction?: ConnectionDirection
) {
	const target = normalizeLoginInput(login);
	const nextTrail = appendTrail(trail, target);
	const search = new URLSearchParams({ trail: nextTrail.join(',') });
	if (direction) search.set('direction', direction);

	return `/app/explore/${encodeURIComponent(target)}?${search.toString()}`;
}

export function buildRepositoryHref(
	fullName: string,
	trail: string[] = [],
	direction?: ConnectionDirection
) {
	const [owner = '', repository = ''] = fullName.split('/', 2);
	const search = new URLSearchParams({ trail: parseTrail(trail.join(',')).join(',') });
	if (direction) search.set('direction', direction);

	return `/app/repository/${encodeURIComponent(owner)}/${encodeURIComponent(repository)}?${search.toString()}`;
}

export function buildTrailHref(
	trail: string[],
	index: number,
	direction?: ConnectionDirection
) {
	const prefix = trail.slice(0, index + 1);
	const login = prefix.at(-1) ?? '';
	return buildExploreHref(login, prefix.slice(0, -1), direction);
}
