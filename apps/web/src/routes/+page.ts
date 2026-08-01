import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent }) => {
	const { authStatus } = await parent();
	throw redirect(302, authStatus.connected ? '/app' : '/login');
};
