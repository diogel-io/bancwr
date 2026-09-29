import { test, expect } from './fixtures'

test('the activity log is empty on a new bunker', async ({ page }) => {
  await page.goto('/logs')
  const table = page.getByRole('table')

  await expect(table.getByRole('columnheader')).toHaveText(['Timestamp', 'Event Kind', 'Member'])
  await expect(table.getByText('No data')).toBeVisible()
})
