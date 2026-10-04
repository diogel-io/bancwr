import { test, expect } from './fixtures'
import type { LogEntry } from '../shared/types/bunker'

test('the activity log lists what the bunker has signed', async ({ page }) => {
  // Connected apps sign during the suite (#31), so it is empty only if nothing has yet.
  const logs = await (await page.request.get('/api/bunker/logs')).json() as LogEntry[]
  await page.goto('/logs')
  const table = page.getByRole('table')

  await expect(table.getByRole('columnheader')).toHaveText(['Timestamp', 'Event Kind', 'Member', 'App'])
  if (logs.length === 0) await expect(table.getByText('No data')).toBeVisible()
  // Body rows only: the table's header has more than one row.
  else await expect(table.locator('tbody tr')).toHaveCount(logs.length)
})

test('the activity log names the member a signature was for, not the app\'s key', async ({ page }) => {
  // diogel-io/workspace#38: Member used to show the app's hex key.
  const logs = await (await page.request.get('/api/bunker/logs')).json() as LogEntry[]
  test.skip(logs.length === 0, 'nothing has been signed yet in this run')
  await page.goto('/logs')
  const rows = page.getByRole('table').locator('tbody tr')

  for (const [i, log] of logs.entries()) {
    const memberCell = rows.nth(i).locator('td').nth(2)
    if (log.member_name) await expect(memberCell).toHaveText(log.member_name)
    await expect(memberCell).not.toHaveText(/^[0-9a-f]{64}$/u)
  }
})
