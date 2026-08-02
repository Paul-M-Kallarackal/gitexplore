import { expect, test } from '@playwright/test';

test('GitHub sign-in is available with the quiet outlined treatment', async ({ page }, testInfo) => {
  await page.route('**/auth/status', async (route) => {
    await route.fulfill({
      json: { authenticated: false, app_user_id: null, connected: false, account: null },
    });
  });

  await page.goto('/login');

  const connect = page.getByRole('link', { name: 'Continue with GitHub' });
  await expect(connect).toBeVisible();
  await expect(connect).toHaveAttribute('href', /\/auth\/oauth\/start\?redirect_to=/);
  await expect(connect).not.toHaveAttribute('aria-disabled', 'true');

  const treatment = await connect.evaluate((element) => {
    const styles = getComputedStyle(element);
    return {
      backgroundColor: styles.backgroundColor,
      borderStyle: styles.borderStyle,
      borderColor: styles.borderColor,
      color: styles.color,
    };
  });

  expect(treatment.backgroundColor).toBe('rgb(255, 255, 255)');
  expect(treatment.borderStyle).toBe('solid');
  expect(treatment.borderColor).not.toBe(treatment.backgroundColor);
  expect(treatment.color).not.toBe(treatment.borderColor);

  await testInfo.attach('login-connect', {
    body: await page.locator('.login-connect').screenshot({ animations: 'disabled' }),
    contentType: 'image/png',
  });
  if (process.env.GITEXPLORE_VISUAL_OUTPUT_DIR) {
    await page.locator('.login-connect').screenshot({
      animations: 'disabled',
      path: `${process.env.GITEXPLORE_VISUAL_OUTPUT_DIR}/login-connect.png`,
    });
  }
});
