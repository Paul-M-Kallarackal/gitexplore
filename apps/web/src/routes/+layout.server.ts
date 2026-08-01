import type { LayoutServerLoad } from './$types';
import { createServerApi, publicApiBaseUrl } from '$lib/server/api';

export const load: LayoutServerLoad = async ({ fetch, url }) => {
	const api = createServerApi(fetch, url.origin);
	const apiBaseUrl = publicApiBaseUrl(url.origin);

	try {
		const authStatus = await api.getAuthStatus();
		return {
			apiBaseUrl,
			authStatus
		};
	} catch {
		return {
			apiBaseUrl,
			authStatus: {
				authenticated: false,
				app_user_id: null,
				connected: false,
				account: null
			}
		};
	}
};
