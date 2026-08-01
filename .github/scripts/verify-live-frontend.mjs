import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';

const rawBaseUrl = process.argv.slice(2).find((argument) => argument !== '--')
  ?? process.env.GITEXPLORE_SMOKE_BASE_URL;
assert(rawBaseUrl, 'Usage: pnpm production:smoke -- <base-url>');

const baseUrl = new URL(rawBaseUrl);
assert(
  baseUrl.protocol === 'https:' || ['localhost', '127.0.0.1', '::1'].includes(baseUrl.hostname),
  'The smoke target must use HTTPS unless it is local',
);

const browser = await chromium.launch({ headless: true });
const failures = [];

try {
  const context = await browser.newContext({ viewport: { width: 375, height: 812 } });
  const page = await context.newPage();

  page.on('console', (message) => {
    if (message.type() === 'error') failures.push(`console: ${message.text()}`);
  });
  page.on('pageerror', (error) => failures.push(`page: ${error.message}`));
  page.on('requestfailed', (request) => {
    if (new URL(request.url()).origin === baseUrl.origin) {
      failures.push(`request: ${request.method()} ${request.url()} (${request.failure()?.errorText ?? 'failed'})`);
    }
  });
  page.on('response', (response) => {
    const request = response.request();
    if (
      new URL(response.url()).origin === baseUrl.origin &&
      ['document', 'script', 'stylesheet'].includes(request.resourceType()) &&
      response.status() >= 400
    ) {
      failures.push(`response: ${response.status()} ${response.url()}`);
    }
  });

  await page.goto(new URL('/login', baseUrl).href, { waitUntil: 'networkidle' });
  await page.getByRole('heading', { name: 'Connect GitHub' }).waitFor();

  const oauthLink = page.getByRole('link', { name: 'Continue with GitHub' });
  await oauthLink.waitFor();
  const oauthHref = await oauthLink.getAttribute('href');
  assert(oauthHref, 'The GitHub OAuth link is missing');
  const oauthUrl = new URL(oauthHref, baseUrl);
  assert.equal(oauthUrl.origin, baseUrl.origin, 'OAuth must remain same-origin');
  assert.equal(oauthUrl.pathname, '/auth/oauth/start', 'OAuth start path is incorrect');
  assert.equal(
    oauthUrl.searchParams.get('redirect_to'),
    new URL('/app/explore', baseUrl).href,
    'OAuth return path is incorrect',
  );

  const visualContract = await page.evaluate(() => {
    const heading = document.querySelector('h1');
    return {
      headingSize: heading ? Number.parseFloat(getComputedStyle(heading).fontSize) : 0,
      overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    };
  });
  assert(visualContract.headingSize >= 32, `Display heading collapsed to ${visualContract.headingSize}px`);
  assert.equal(visualContract.overflow, false, 'Login page overflows a 375px viewport');

  await page.goto(new URL('/app/explore', baseUrl).href, { waitUntil: 'domcontentloaded' });
  await page.waitForURL(new URL('/login', baseUrl).href);
  await page.getByRole('heading', { name: 'Connect GitHub' }).waitFor();

  assert.deepEqual(failures, [], failures.join('\n'));
  await context.close();
} finally {
  await browser.close();
}

console.log(`Live React frontend verified at ${baseUrl.origin}`);
