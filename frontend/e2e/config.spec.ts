import { test, expect } from './fixtures'

test('shows the bunker public key, and that the signing key is encrypted at rest (#94)', async ({ page, bunkerNpub }) => {
  test.skip(!bunkerNpub, 'The bunker key is unknown: set BUNKER_NSEC when using E2E_BASE_URL.')

  await page.goto('/config')
  await expect(page.getByRole('definition').filter({ hasText: 'npub1' })).toHaveText(bunkerNpub)
  await expect(page.getByTestId('key-storage')).toHaveText('Encrypted at rest')
})

test('the key is read-only (#42)', async ({ page }) => {
  await page.goto('/config')
  // UDashboardPanel id="config" renders as #dashboard-panel-config.
  const panel = page.locator('#dashboard-panel-config')
  await expect(panel).toBeVisible()
  await expect(page.getByText('Current Pubkey')).toBeVisible()

  // The key's card takes no input. The log retention card below it is a form (#96).
  const keyCard = page.getByTestId('key-card')
  await expect(keyCard.getByRole('textbox')).toHaveCount(0)
  await expect(keyCard.getByRole('combobox')).toHaveCount(0)
  await expect(keyCard.locator('form, button[type="submit"]')).toHaveCount(0)
})

test('shows and saves the log retention policy (#96)', async ({ page }) => {
  await page.goto('/config')
  const card = page.getByTestId('log-retention')
  await expect(card.getByTestId('log-retention-current')).toHaveText('Kept indefinitely')

  // Under the floor: refused before anything is sent.
  await card.getByTestId('log-retention-days').fill('7')
  await expect(card.getByText('At least 30 days, or leave it empty.')).toBeVisible()
  await expect(card.getByTestId('log-retention-save')).toBeDisabled()

  // Ten years prunes nothing the other specs made.
  await card.getByTestId('log-retention-days').fill('3650')
  await card.getByTestId('log-retention-save').click()
  await expect(card.getByTestId('log-retention-saved')).toBeVisible()
  await expect(card.getByTestId('log-retention-current')).toHaveText('Kept for 3,650 days')
  await page.reload()
  await expect(page.getByTestId('log-retention-current')).toHaveText('Kept for 3,650 days')

  // Back to the default, for the specs after this one.
  await page.getByTestId('log-retention-days').fill('')
  await page.getByTestId('log-retention-save').click()
  await expect(page.getByTestId('log-retention-current')).toHaveText('Kept indefinitely')
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
