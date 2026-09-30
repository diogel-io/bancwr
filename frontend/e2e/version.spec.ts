import { test, expect } from './fixtures'

// compose.e2e.yaml gives both containers this BANCWR_VERSION (#35).
const VERSION = '0.0.0-e2e'

test('the bunker and the frontend report the version they are running', async ({ page }) => {
  // page.request carries the signed-in session; /api/bunker/* needs one (#11).
  const status = await (await page.request.get('/api/bunker/status')).json()
  expect(status.version).toBe(VERSION)

  const frontend = await (await page.request.get('/api/version')).json()
  expect(frontend.version).toBe(VERSION)
})

test('the sidebar footer shows both versions', async ({ page }) => {
  await page.goto('/')
  const versions = page.getByRole('group', { name: /^Bunker / })

  await expect(versions).toContainText(`Bunker ${VERSION}`)
  await expect(versions).toContainText(`Frontend ${VERSION}`)
  await expect(versions).not.toContainText('differ')
})
