import type { HandleFetch } from '@sveltejs/kit';
import { internalApiOrigin } from '$lib/server/api';

export const handleFetch: HandleFetch = async ({ event, request, fetch }) => {
	const apiOrigin = internalApiOrigin(event.url.origin);
	const requestOrigin = new URL(request.url).origin;
	const sessionId = event.cookies.get('gitexplore_session');

	if (apiOrigin && requestOrigin === apiOrigin && sessionId) {
		const headers = new Headers(request.headers);
		const requestCookies = headers.get('cookie') ?? '';

		if (!/(?:^|;\s*)gitexplore_session=/.test(requestCookies)) {
			const sessionCookie = `gitexplore_session=${sessionId}`;
			headers.set('cookie', requestCookies ? `${requestCookies}; ${sessionCookie}` : sessionCookie);
			request = new Request(request, { headers });
		}
	}

	return fetch(request);
};
