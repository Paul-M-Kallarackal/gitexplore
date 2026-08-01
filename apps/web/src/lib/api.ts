import { env } from '$env/dynamic/public';
import { createGitExploreApi } from '@gitexplore/api-client';

function runtimeApiBaseUrl() {
	if (env.PUBLIC_GITEXPLORE_API_BASE_URL) return env.PUBLIC_GITEXPLORE_API_BASE_URL;
	if (typeof window !== 'undefined') return window.location.origin;
	throw new Error('No browser API origin is available');
}

export function createBrowserApi(baseUrl?: string) {
	return createGitExploreApi({ baseUrl: baseUrl || runtimeApiBaseUrl() });
}
