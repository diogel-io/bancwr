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

test('the sidebar header shows the release version under the name', async ({ page }) => {
  await page.goto('/')

  await expect(page.getByTestId('app-version')).toHaveText(`V ${VERSION}`)
  // One version now (#124): the footer's bunker and frontend lines are gone.
  await expect(page.getByText(`Bunker ${VERSION}`)).toHaveCount(0)
  await expect(page.getByText(`Frontend ${VERSION}`)).toHaveCount(0)
})
