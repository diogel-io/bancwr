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

  // As the bunker reports them. Other specs now connect apps and sign (#31), so these are not
  // zero; they do not change while this test runs.
  const metrics = await (await page.request.get('/api/bunker/metrics')).json() as { total_signatures: number, nip46_connections: number }
  await expect(metric(page, 'Total Signatures')).toHaveText(String(metrics.total_signatures))
  await expect(metric(page, 'NIP-46 Connections')).toHaveText(String(metrics.nip46_connections))

  // The bunker counts status requests, and global setup made several before this page loaded, so
  // a non-zero count shows the numbers came from the running bunker, not from the defaults.
  const requests = Number(await metric(page, 'HTTP Requests').textContent())
  expect(requests).toBeGreaterThan(0)
})

test('shows the bunker\'s recent activity', async ({ page }) => {
  // Connected apps sign during the suite (#31), so whether there is any depends on what ran first.
  const logs = await (await page.request.get('/api/bunker/logs')).json() as { event_id: string }[]
  const activity = card(page, 'Recent Activity')
  await expect(activity.getByRole('table')).toBeVisible()
  if (logs.length === 0) await expect(activity.getByText('No data')).toBeVisible()
  // Body rows only: the table's header has more than one row.
  else await expect(activity.locator('tbody tr')).toHaveCount(Math.min(logs.length, 5))
})

/** The UCard whose header holds this heading. */
function card(page: Page, heading: string) {
  return page.getByRole('heading', { name: heading }).locator('xpath=ancestor::*[@data-slot="root"][1]')
}

/** The value shown above a metric card's label. */
function metric(page: Page, label: string) {
  return page.getByText(label, { exact: true }).locator('xpath=preceding-sibling::p[1]')
}
