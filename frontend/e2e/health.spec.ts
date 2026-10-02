import type { Page, TestInfo } from '@playwright/test'
import { test, expect, type Role } from './fixtures'

// The health indicator in every page's header (#28), on the real stack. NIP-46 is on there (#31),
// with the in-process relay, so the bunker is healthy with its relay check passing (#27). Red and
// yellow are served to the browser with page.route, then fetched with "Check now", since a real
// bunker cannot be made degraded on demand here.

const indicator = (page: Page) => page.getByTestId('bunker-health')
const detail = (page: Page) => page.getByTestId('bunker-health-detail')

// Administrators open every page; the others are refused three of them, and the permission-denied
// page carries the same header.
const PAGES: Record<Role, string[]> = {
  administrator: ['/', '/config', '/team', '/logs'],
  user: ['/', '/config'],
  signer: ['/', '/logs']
}

for (const role of ['administrator', 'user', 'signer'] as Role[]) {
  test.describe(role, () => {
    test.use({ role })

    test('sees the bunker healthy in the header of every page it reaches', async ({ page }) => {
      for (const path of PAGES[role]) {
        await page.goto(path)
        await expect(indicator(page), path).toHaveAccessibleName('Bunker health: Healthy. Show details')
        await expect(indicator(page), path).toHaveAttribute('data-state', 'healthy')
      }
    })
  })
}

test('the detail lists each check, with the bunker\'s NIP-46 relay connected (#31)', async ({ page }) => {
  await page.goto('/team')
  await indicator(page).click()

  await expect(detail(page)).toContainText('Bunker health: Healthy')
  for (const [check, state] of [['signer', 'OK'], ['database', 'OK'], ['relays', 'OK']] as const) {
    await expect(detail(page).locator(`[data-check="${check}"]`)).toContainText(state)
  }
  await expect(detail(page)).toContainText('All 1 relays connected.')
  await expect(detail(page)).toContainText(/Last checked \d+ s ago/)
})

test('a bunker that does not answer is red, and says so', async ({ page }, testInfo) => {
  await page.goto('/logs')
  await page.route('**/api/bunker/status', route => route.abort())

  await indicator(page).click()
  await detail(page).getByRole('button', { name: 'Check now' }).click()

  await expect(indicator(page)).toHaveAccessibleName('Bunker health: Down. Show details')
  await expect(detail(page)).toContainText('The bunker did not answer')
  await screenshots(page, testInfo, 'down')
})

test('a degraded bunker is yellow, and names what needs attention', async ({ page }, testInfo) => {
  await page.goto('/config')
  await page.route('**/api/bunker/status', route => route.fulfill({
    json: {
      status: 'degraded',
      pubkey: process.env.E2E_BUNKER_NPUB ?? 'npub1test',
      version: '0.0.0',
      checks: [
        { name: 'signer', status: 'pass', detail: 'The signing key signs and verifies.' },
        { name: 'database', status: 'pass', detail: 'The database answers.' },
        {
          name: 'relays',
          status: 'warn',
          detail: '1 of 2 relays connected. Not connected: wss://relay.example',
          relays: [{ url: 'wss://relay.up.example', connected: true }, { url: 'wss://relay.example', connected: false }]
        }
      ]
    }
  }))

  await indicator(page).click()
  await detail(page).getByRole('button', { name: 'Check now' }).click()

  await expect(indicator(page)).toHaveAccessibleName('Bunker health: Degraded. Show details')
  await expect(detail(page).locator('[data-check="relays"]')).toContainText('Needs attention')
  await expect(detail(page)).toContainText('Not connected: wss://relay.example')
  await screenshots(page, testInfo, 'degraded')
})

test('healthy, in both colour schemes', async ({ page }, testInfo) => {
  await page.goto('/')
  await indicator(page).click()
  await expect(detail(page)).toBeVisible()
  await screenshots(page, testInfo, 'healthy')
})

/** The header and open detail in light and dark, attached to the report for a contrast check. */
async function screenshots(page: Page, testInfo: TestInfo, state: string) {
  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme })
    await testInfo.attach(`${state}-${colorScheme}`, {
      body: await page.screenshot({ clip: { x: 0, y: 0, width: 1280, height: 480 } }),
      contentType: 'image/png'
    })
  }
}
