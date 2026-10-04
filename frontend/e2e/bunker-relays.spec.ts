// The bunker's own relays (#78) through the real stack. compose.e2e.yaml sets NIP46_RELAYS, which
// always decides, so the console shows that list read-only and the bunker refuses to change it
// (409 relays_from_environment), through the frontend proxy as the console would send it.
import { test, expect } from './fixtures'

// The relay as the bunker container names it (compose.e2e.yaml).
const envRelay = `ws://e2e-host:${process.env.E2E_RELAY_PORT || 7777}`

test('the Config page shows the NIP46_RELAYS relays read-only, with their status', async ({ page }) => {
  await page.goto('/config')
  const section = page.getByTestId('bunker-relays-section')
  await expect(section.getByRole('heading', { name: 'Bunker relays' })).toBeVisible()
  await expect(section.getByTestId('bunker-relays-read-only')).toContainText('NIP46_RELAYS')

  const row = section.locator(`[data-relay="${envRelay}"]`)
  await expect(row).toBeVisible()
  // The bunker connects to the in-process relay at startup.
  await expect(async () => {
    await page.reload()
    await expect(page.getByTestId('bunker-relays-section').locator(`[data-relay="${envRelay}"]`)).toContainText('Connected', { timeout: 1000 })
  }).toPass({ timeout: 15_000 })

  // Nothing to change: no address field, no search, no save.
  await expect(section.getByRole('textbox')).toHaveCount(0)
  await expect(section.getByRole('button')).toHaveCount(0)
  await expect(section.locator('form')).toHaveCount(0)
})

test('the bunker refuses to change relays NIP46_RELAYS sets (409), and keeps them', async ({ page }) => {
  const res = await page.request.put('/api/bunker/relays', { data: { relays: ['wss://relay.example.com'] } })
  expect(res.status()).toBe(409)
  // The bunker's { error, message }, as the proxy forwards it.
  expect(JSON.stringify(await res.json())).toContain('relays_from_environment')

  const listed = await page.request.get('/api/bunker/relays')
  expect(listed.status()).toBe(200)
  expect(await listed.json()).toMatchObject({ source: 'environment', nip46_enabled: true, relays: [{ url: envRelay }] })
})

test.describe('a user', () => {
  test.use({ role: 'user' })

  test('can neither read nor change the bunker relays', async ({ page }) => {
    expect((await page.request.get('/api/bunker/relays')).status()).toBe(403)
    expect((await page.request.put('/api/bunker/relays', { data: { relays: ['wss://relay.example.com'] } })).status()).toBe(403)
  })
})
