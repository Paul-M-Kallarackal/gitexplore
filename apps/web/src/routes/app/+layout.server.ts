import { redirect } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';
import { createServerApi } from '$lib/server/api';

export const load: LayoutServerLoad = async ({ parent, fetch, url }) => {
	const { authStatus, apiBaseUrl } = await parent();

	if (!authStatus.connected) {
		throw redirect(302, '/login');
	}

	const api = createServerApi(fetch, url.origin);

	try {
		const syncStatus = await api.getSyncStatus();
		return {
			authStatus,
			apiBaseUrl,
			syncStatus
		};
	} catch {
		return {
			authStatus,
			apiBaseUrl,
			syncStatus: {
				state: 'NeverSynced' as const,
				last_synced_at: null,
				last_error: null
			}
		};
	}
};
