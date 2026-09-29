import { test, expect } from './fixtures'

test('shows the bunker public key and where the signing key comes from', async ({ page, bunkerNpub }) => {
  test.skip(!bunkerNpub, 'The bunker key is unknown: set BUNKER_NSEC when using E2E_BASE_URL.')

  await page.goto('/config')
  await expect(page.getByRole('definition').filter({ hasText: 'npub1' })).toHaveText(bunkerNpub)
  await expect(page.getByText('Environment variable: BUNKER_NSEC')).toBeVisible()
})

test('is read-only (#42)', async ({ page }) => {
  await page.goto('/config')
  // UDashboardPanel id="config" renders as #dashboard-panel-config.
  const panel = page.locator('#dashboard-panel-config')
  await expect(panel).toBeVisible()
  await expect(page.getByText('Current Pubkey')).toBeVisible()

  await expect(panel.getByRole('textbox')).toHaveCount(0)
  await expect(panel.getByRole('combobox')).toHaveCount(0)
  await expect(panel.locator('form, button[type="submit"]')).toHaveCount(0)
})

test('never exposes a secret key', async ({ page }) => {
  const configBodies: string[] = []
  page.on('response', async (response) => {
    if (response.url().includes('/api/bunker/config')) {
      configBodies.push(await response.text())
    }
  })

  // Server-rendered, so the payload carries the config response too.
  const response = await page.goto('/config')
  expect(await response?.text()).not.toContain('nsec1')

  // And after client-side navigation, which fetches the config from the browser.
  await page.goto('/')
  await page.getByRole('navigation').getByRole('link', { name: 'Config', exact: true }).click()
  await expect(page.getByText('Current Pubkey')).toBeVisible()
  expect(await page.content()).not.toContain('nsec1')
  for (const body of configBodies) {
    expect(body).not.toContain('nsec1')
  }
})
