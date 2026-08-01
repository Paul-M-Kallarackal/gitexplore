import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

type Progress = {
  version: number;
  status: 'NOT_STARTED' | 'IN_PROGRESS' | 'COMPLETED' | 'DISMISSED';
  startedAt: string | null;
  completedAt: string | null;
  dismissedAt: string | null;
  openedTrailhead: boolean;
  followedConnection: boolean;
  savedRepository: boolean;
  mappingStarted: boolean;
};

const alice = {
  githubId: '1', login: 'alice', name: 'Alice', url: 'https://github.com/alice',
  avatarUrl: null, bio: 'Builds graph tools.', followersCount: 12, followingCount: 8,
};
const bob = {
  githubId: '2', login: 'bob', name: 'Bob', url: 'https://github.com/bob',
  avatarUrl: null, bio: 'Maintains useful things.', followersCount: 20, followingCount: 11,
};
const repository = {
  githubId: '10', ownerLogin: 'acme', name: 'trail-map', fullName: 'acme/trail-map',
  description: 'A small tool for navigating open source.', htmlUrl: 'https://github.com/acme/trail-map',
  stargazerCount: 84, forkCount: 6, primaryLanguage: 'Rust', topics: ['graph'],
  updatedAt: '2026-08-01T09:00:00Z', archived: false, fork: false,
};

function neighborhood(user: typeof alice, withRepository: boolean) {
  return {
    user,
    followers: user.login === 'alice' ? [bob] : [],
    following: [],
    repositories: withRepository ? [{
      repository,
      networkStars: 3,
      viaLogins: ['bob'],
      discoveryScore: 42,
      reasons: ['Starred by people near this trail'],
      saved: false,
    }] : [],
    cacheStatus: 'FRESH',
    lastFetchedAt: '2026-08-01T09:00:00Z',
    coverage: {
      followersComplete: true,
      followingComplete: true,
      starredRepositoriesComplete: true,
      repositoriesComplete: true,
    },
  };
}

async function mockGitExplore(page: Page) {
  const startedAt = '2026-08-01T10:00:00Z';
  const visits: Array<{ user: typeof alice; trail: string[]; direction: 'FOLLOWERS' | 'FOLLOWING'; lastViewedAt: string; visitCount: number; visible: boolean }> = [];
  let saved = false;
  let progress: Progress = {
    version: 1,
    status: 'NOT_STARTED',
    startedAt: null,
    completedAt: null,
    dismissedAt: null,
    openedTrailhead: false,
    followedConnection: false,
    savedRepository: false,
    mappingStarted: false,
  };

  await page.route('**/auth/status', (route) => route.fulfill({
    json: {
      authenticated: true,
      app_user_id: 'app-user-1',
      connected: true,
      account: { github_user_id: 1, login: 'alice', display_name: 'Alice' },
    },
  }));
  await page.route('**/bookmarks', async (route, request) => {
    if (request.method() === 'GET') {
      await route.fulfill({ json: saved ? [{
        id: 'bookmark-1',
        target: { GitHubRepository: { full_name: repository.fullName } },
        categories: [],
        note: null,
        created_at: '2026-08-01T10:05:00Z',
      }] : [] });
      return;
    }
    await route.fallback();
  });
  await page.route('**/categories', (route) => route.fulfill({ json: [] }));
  await page.route('**/graphql', async (route, request) => {
    const body = request.postDataJSON() as { query: string; variables: Record<string, unknown> };
    const query = body.query;
    let data: Record<string, unknown>;

    if (query.includes('mutation BeginOnboarding')) {
      progress = { ...progress, status: 'IN_PROGRESS', startedAt };
      data = { beginOnboarding: progress };
    } else if (query.includes('mutation DismissOnboarding')) {
      progress = { ...progress, status: 'DISMISSED', dismissedAt: '2026-08-01T10:01:00Z' };
      data = { dismissOnboarding: progress };
    } else if (query.includes('mutation CompleteOnboarding')) {
      progress = { ...progress, status: 'COMPLETED', completedAt: '2026-08-01T10:05:00Z' };
      data = { completeOnboarding: progress };
    } else if (query.includes('query OnboardingProgress')) {
      data = { onboardingProgress: progress };
    } else if (query.includes('query DiscoveryWarmup')) {
      data = { discoveryWarmup: null };
    } else if (query.includes('query RateLimit')) {
      data = { rateLimit: { limit: 5000, used: 800, remaining: 4200, resetAt: '2026-08-01T11:00:00Z', checkedAt: startedAt } };
    } else if (query.includes('mutation RecordPersonVisit')) {
      const login = String(body.variables.login);
      const user = login === 'alice' ? alice : bob;
      const trail = body.variables.trail as string[];
      visits.unshift({ user, trail, direction: String(body.variables.direction) as 'FOLLOWERS' | 'FOLLOWING', lastViewedAt: '2026-08-01T10:02:00Z', visitCount: 1, visible: true });
      progress = {
        ...progress,
        openedTrailhead: visits.length > 0,
        followedConnection: visits.some((visit) => visit.trail.length >= 2),
      };
      data = { recordPersonVisit: { recentPeople: visits, maxTrailDepth: progress.followedConnection ? 1 : 0 } };
    } else if (query.includes('query ExplorationActivity')) {
      data = { explorationActivity: { recentPeople: visits, maxTrailDepth: progress.followedConnection ? 1 : 0 } };
    } else if (query.includes('query Neighborhood')) {
      const login = String(body.variables.login);
      data = { neighborhood: neighborhood(login === 'alice' ? alice : bob, login === 'bob') };
    } else if (query.includes('mutation SaveRepository')) {
      saved = true;
      progress = { ...progress, savedRepository: true };
      data = { saveRepository: { id: 'bookmark-1', fullName: repository.fullName, categories: [], note: null, createdAt: '2026-08-01T10:05:00Z' } };
    } else if (query.includes('query UserInsights')) {
      data = { userInsights: { login: String(body.variables.login), repositories: [], windowDays: 90, sourceEventCount: 0, sourceTruncated: false, sourceDescription: 'Public events', cacheStatus: 'FRESH', lastFetchedAt: startedAt } };
    } else {
      throw new Error(`Unhandled GraphQL operation: ${query}`);
    }
    await route.fulfill({ json: { data } });
  });
}

test('connected user reaches a private first save through the onboarding trail', async ({ page }) => {
  await mockGitExplore(page);
  await page.goto('/app/explore');

  await expect(page.getByRole('heading', { name: 'Your first GitExplore trail' })).toBeVisible();
  expect((await new AxeBuilder({ page }).include('.onboarding-card').analyze()).violations).toEqual([]);

  await page.getByRole('link', { name: 'Start from @alice' }).click();
  await expect(page.getByRole('heading', { name: 'Alice' })).toBeVisible();
  await page.getByRole('link', { name: /Bob @bob/ }).click();
  await expect(page.getByRole('heading', { name: 'Bob' })).toBeVisible();
  await page.getByRole('button', { name: 'Save' }).click();

  await expect(page.getByText('Your first find is saved')).toBeVisible();
  expect((await new AxeBuilder({ page }).include('#main-content').analyze()).violations).toEqual([]);
  await page.getByRole('link', { name: 'Open Saved' }).click();
  await expect(page.getByRole('heading', { name: 'Saved' })).toBeVisible();
  await expect(page.getByText('acme/trail-map')).toBeVisible();
});

test('skip remains dismissed after a reload', async ({ page }) => {
  await mockGitExplore(page);
  await page.goto('/app/explore');

  await page.getByRole('button', { name: 'Skip onboarding' }).click();
  await expect(page.getByRole('heading', { name: 'Your first GitExplore trail' })).toBeHidden();
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Your first GitExplore trail' })).toBeHidden();
  await expect(page.getByRole('heading', { name: /Whose GitHub world/ })).toBeVisible();
});

test('atlas onboarding stays composed without cut-through connectors', async ({ page }, testInfo) => {
  await mockGitExplore(page);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/app/explore');

  const card = page.locator('.onboarding-card');
  const artwork = card.locator('.onboarding-art img');
  await expect(card).toBeVisible();
  await expect(artwork).toBeVisible();
  await expect.poll(() => artwork.evaluate((image) => (image as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);

  const connectorContent = await card.locator('.onboarding-steps li').evaluateAll((items) => (
    items.map((item) => getComputedStyle(item, '::after').content)
  ));
  expect(connectorContent.every((content) => content === 'none')).toBe(true);

  const cardBox = await card.boundingBox();
  const artworkBox = await card.locator('.onboarding-art').boundingBox();
  expect(cardBox).not.toBeNull();
  expect(artworkBox).not.toBeNull();
  expect(artworkBox!.x).toBeGreaterThan(cardBox!.x + cardBox!.width * 0.55);
  await testInfo.attach('onboarding-desktop', {
    body: await card.screenshot({ animations: 'disabled' }),
    contentType: 'image/png',
  });
  if (process.env.GITEXPLORE_VISUAL_OUTPUT_DIR) {
    await card.screenshot({
      animations: 'disabled',
      path: `${process.env.GITEXPLORE_VISUAL_OUTPUT_DIR}/onboarding-desktop.png`,
    });
  }

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(card).toBeVisible();
  const mobileArtBox = await card.locator('.onboarding-art').boundingBox();
  const mobileBodyBox = await card.locator('.onboarding-card-body').boundingBox();
  expect(mobileArtBox).not.toBeNull();
  expect(mobileBodyBox).not.toBeNull();
  expect(mobileArtBox!.y).toBeLessThan(mobileBodyBox!.y);
  expect((await new AxeBuilder({ page }).include('.onboarding-card').analyze()).violations).toEqual([]);
  await page.evaluate(() => window.scrollTo(0, 0));
  await testInfo.attach('onboarding-mobile', {
    body: await page.screenshot({ animations: 'disabled', fullPage: true }),
    contentType: 'image/png',
  });
  if (process.env.GITEXPLORE_VISUAL_OUTPUT_DIR) {
    await page.screenshot({
      animations: 'disabled',
      fullPage: true,
      path: `${process.env.GITEXPLORE_VISUAL_OUTPUT_DIR}/onboarding-mobile.png`,
    });
  }
});
