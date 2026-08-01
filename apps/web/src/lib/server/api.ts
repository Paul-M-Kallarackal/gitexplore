import { env as privateEnv } from '$env/dynamic/private';
import { env as publicEnv } from '$env/dynamic/public';
import { createGitExploreApi, type FetchLike } from '@gitexplore/api-client';

const configuredPublicApiBaseUrl = publicEnv.PUBLIC_GITEXPLORE_API_BASE_URL;
const configuredInternalApiBaseUrl = privateEnv.GITEXPLORE_INTERNAL_API_BASE_URL;

export function publicApiBaseUrl(requestOrigin: string) {
	return configuredPublicApiBaseUrl || requestOrigin;
}

export function internalApiBaseUrl(requestOrigin: string) {
	return configuredInternalApiBaseUrl || publicApiBaseUrl(requestOrigin);
}

export function createServerApi(fetch: FetchLike, requestOrigin: string) {
	return createGitExploreApi({ baseUrl: internalApiBaseUrl(requestOrigin), fetch });
}

export function internalApiOrigin(requestOrigin: string) {
	try {
		return new URL(internalApiBaseUrl(requestOrigin)).origin;
	} catch {
		return null;
	}
}
