import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent, url }) => {
	const { authStatus, apiBaseUrl } = await parent();

	if (authStatus.connected) {
		throw redirect(302, '/app');
	}

	return {
		apiBaseUrl,
		appOrigin: url.origin
	};
};
