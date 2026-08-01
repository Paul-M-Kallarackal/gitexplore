import { createGitExploreApi } from '@gitexplore/api-client';

export function createBrowserApi() {
  return createGitExploreApi({ baseUrl: window.location.origin });
}

export const api = createBrowserApi();
