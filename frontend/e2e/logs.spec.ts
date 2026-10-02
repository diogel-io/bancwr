import { test, expect } from './fixtures'

test('the activity log lists what the bunker has signed', async ({ page }) => {
  // Connected apps sign during the suite (#31), so it is empty only if nothing has yet.
  const logs = await (await page.request.get('/api/bunker/logs')).json() as { event_id: string }[]
  await page.goto('/logs')
  const table = page.getByRole('table')

  await expect(table.getByRole('columnheader')).toHaveText(['Timestamp', 'Event Kind', 'Member'])
  if (logs.length === 0) await expect(table.getByText('No data')).toBeVisible()
  // Body rows only: the table's header has more than one row.
  else await expect(table.locator('tbody tr')).toHaveCount(logs.length)
})
