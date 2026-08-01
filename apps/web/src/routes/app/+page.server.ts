import { redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ parent }) => {
	const { authStatus } = await parent();
	const login = authStatus.account?.login;

	throw redirect(302, login ? `/app/explore/${encodeURIComponent(login)}` : '/app/explore');
};
