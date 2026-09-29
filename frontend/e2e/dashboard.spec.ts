import type { Page } from '@playwright/test'
import { test, expect } from './fixtures'

test.beforeEach(async ({ page }) => {
  await page.goto('/')
})

test('shows the bunker as healthy, with its public key', async ({ page, bunkerNpub }) => {
  const status = card(page, 'Bunker Status')
  await expect(status.getByText('healthy', { exact: true })).toBeVisible()
  if (bunkerNpub) {
    await expect(status.getByText(bunkerNpub)).toBeAttached()
  }
})

test('shows the metrics from the live bunker', async ({ page }) => {
  for (const label of ['Total Signatures', 'NIP-46 Connections', 'HTTP Requests']) {
    await expect(page.getByText(label, { exact: true })).toBeVisible()
  }

  // No signing happens in this suite, so these stay at zero.
  await expect(metric(page, 'Total Signatures')).toHaveText('0')
  await expect(metric(page, 'NIP-46 Connections')).toHaveText('0')

  // The bunker counts status requests, and global setup made several before this page loaded, so
  // a non-zero count shows the numbers came from the running bunker, not from the defaults.
  const requests = Number(await metric(page, 'HTTP Requests').textContent())
  expect(requests).toBeGreaterThan(0)
})

test('shows no recent activity on a new bunker', async ({ page }) => {
  const activity = card(page, 'Recent Activity')
  await expect(activity.getByRole('table')).toBeVisible()
  await expect(activity.getByText('No data')).toBeVisible()
})

/** The UCard whose header holds this heading. */
function card(page: Page, heading: string) {
  return page.getByRole('heading', { name: heading }).locator('xpath=ancestor::*[@data-slot="root"][1]')
}

/** The value shown above a metric card's label. */
function metric(page: Page, label: string) {
  return page.getByText(label, { exact: true }).locator('xpath=preceding-sibling::p[1]')
}
