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

test('the log pages beyond its first page, continuing from the last row (#96)', async ({ page }) => {
  const first = await (await page.request.get('/api/bunker/logs?limit=1')).json() as LogEntry[]
  test.skip(first.length === 0, 'nothing has been signed yet in this run')
  const last = first[0]!
  const before = encodeURIComponent(`${last.timestamp}|${last.id}`)
  const next = await (await page.request.get(`/api/bunker/logs?limit=500&before=${before}`)).json() as LogEntry[]
  const all = await (await page.request.get('/api/bunker/logs?limit=500')).json() as LogEntry[]
  expect(next.map(l => l.id)).toEqual(all.slice(1).map(l => l.id))
})

test('an administrator downloads the log and audit events as CSV, and the export is recorded (#96)', async ({ page }) => {
  await page.goto('/logs')
  const today = new Date().toISOString().slice(0, 10)
  await page.getByTestId('log-export-from').fill(today)
  await page.getByTestId('log-export-to').fill(today)

  const download = page.waitForEvent('download')
  await page.getByTestId('log-export-download').click()
  const file = await download
  expect(file.suggestedFilename()).toBe(`bancwr-log-${today}-${today}.csv`)

  const csv = await (await page.request.get(`/api/bunker/logs/export?format=csv&from=${today}&to=${today}`)).text()
  const lines = csv.split('\r\n').filter(Boolean)
  expect(lines[0]).toBe('time,source,action,event_kind,event_id,member_pubkey,member_name,connection_id,token_id,client_pubkey,actor_pubkey,detail')
  // The download above was recorded before this one read its rows.
  expect(lines.some(l => l.includes(',audit,logs_exported,') && l.endsWith(`format=csv;from=${today};to=${today}`))).toBe(true)
})

test('a reversed export range is refused in the page and by the bunker (#96)', async ({ page }) => {
  await page.goto('/logs')
  await page.getByTestId('log-export-from').fill('2026-06-02')
  await page.getByTestId('log-export-to').fill('2026-06-01')
  await expect(page.getByTestId('log-export-error')).toHaveText('The from date must not be after the to date.')

  const refused = await page.request.get('/api/bunker/logs/export?format=csv&from=2026-06-02&to=2026-06-01')
  expect(refused.status()).toBe(400)
})
