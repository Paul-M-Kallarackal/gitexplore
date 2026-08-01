const githubLoginPattern = /^[a-z\d](?:[a-z\d-]{0,37}[a-z\d])?$/i;
export const maxTrailEntries = 8;

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
	const trail = (value ?? '')
		.split(',')
		.map(normalizeLoginInput)
		.filter((login) => githubLoginPattern.test(login));

	return appendTrail(trail, current);
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

export function buildExploreHref(login: string, trail: string[] = []) {
	const target = normalizeLoginInput(login);
	const nextTrail = appendTrail(trail, target);
	const search = new URLSearchParams({ trail: nextTrail.join(',') });

	return `/app/explore/${encodeURIComponent(target)}?${search.toString()}`;
}

export function buildTrailHref(trail: string[], index: number) {
	const prefix = trail.slice(0, index + 1);
	const login = prefix.at(-1) ?? '';
	return buildExploreHref(login, prefix.slice(0, -1));
}
