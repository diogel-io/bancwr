// The sidebar footer's avatar (#72), through the real stack: the signed-in key's profile picture
// from the in-process relay, or the user icon. Each test registers a fresh key of its own, so the
// profiles other specs rely on are left alone. The image host is answered with page.route.
import type { Page, Route } from '@playwright/test'
import { SimplePool } from 'nostr-tools/pool'
import { finalizeEvent } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'
import { continueAsKey, installExtension } from './extension'
import { anonymousTest, expect } from './fixtures'
import { generateNsec, npubFromNsec } from './keys'
import { cookieJarPost, signInAs } from './sign-in'
import { baseUrl } from './stack'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`
const pool = new SimplePool()

// A 1x1 PNG, enough for the browser to load it.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64')

/** A new key registered as a user by the administrator, with no profile anywhere yet. */
async function newMember(): Promise<string> {
  const nsec = generateNsec()
  const jar = cookieJarPost()
  await signInAs(process.env.E2E_ADMIN_NSEC!, baseUrl(), jar.post)
  const added = await jar.post(`${baseUrl()}/api/bunker/team`, {
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: `avatar ${Date.now()}`, pubkey: npubFromNsec(nsec), role: 'user' })
  })
  expect(added.status).toBe(200)
  return nsec
}

async function seedProfile(nsec: string, content: Record<string, unknown>) {
  const event = finalizeEvent({ kind: 0, created_at: Math.floor(Date.now() / 1000) - 60, tags: [], content: JSON.stringify(content) }, decode(nsec).data as Uint8Array)
  await Promise.any(pool.publish([relay], event))
}

/**
 * Serves every picture on the test image host, recording each request's referrer. The sign-in
 * confirmation (#70) loads the picture first, and the footer may reuse that cached copy, so both
 * must send none.
 */
async function serveImages(page: Page): Promise<string[]> {
  const referrers: string[] = []
  await page.route('https://img.e2e.test/**', async (route: Route) => {
    referrers.push((await route.request().allHeaders()).referer ?? '')
    await route.fulfill({ body: PNG, contentType: 'image/png' })
  })
  return referrers
}

async function signInWithExtension(page: Page, nsec: string) {
  await installExtension(page, nsec)
  await page.goto('/sign-in')
  await page.getByRole('button', { name: 'Sign in with extension' }).click()
  await continueAsKey(page)
  await expect(page).toHaveURL('/')
}

const avatarImage = (page: Page) => page.locator('img[data-testid="user-avatar"]')

anonymousTest('the footer shows the signed-in key\'s picture and name, with no referrer', async ({ page }) => {
  const nsec = await newMember()
  await seedProfile(nsec, { name: 'avatar-user', display_name: 'Avatar User', picture: 'https://img.e2e.test/me.png' })
  const referrers = await serveImages(page)
  await signInWithExtension(page, nsec)

  await expect(avatarImage(page)).toHaveAttribute('src', 'https://img.e2e.test/me.png')
  await expect.poll(() => avatarImage(page).evaluate(img => (img as HTMLImageElement).naturalWidth)).toBe(1)
  await expect(page.getByTestId('user-name')).toHaveText('Avatar User')
  await expect(page.getByTestId('user-name')).toHaveAttribute('title', npubFromNsec(nsec))
  expect(referrers.length).toBeGreaterThan(0)
  expect(referrers.every(referrer => referrer === '')).toBe(true)
})

anonymousTest('a key with no profile gets the user icon, and no image', async ({ page }) => {
  const nsec = await newMember()
  await signInWithExtension(page, nsec)

  // The lookup gives up after a few seconds; the short npub stays, and no image appears.
  const npub = npubFromNsec(nsec)
  await expect(page.getByTestId('user-name')).toHaveText(`${npub.slice(0, 12)}…${npub.slice(-4)}`)
  await page.waitForTimeout(3500)
  await expect(avatarImage(page)).toHaveCount(0)
  // Without a picture, UAvatar renders the icon itself, carrying the test id.
  await expect(page.locator('[data-testid="user-avatar"]:not(img)')).toHaveClass(/i-lucide:user/)
})

anonymousTest('changing the picture on /profile updates the footer without a reload', async ({ page }) => {
  const nsec = await newMember()
  await seedProfile(nsec, { name: 'before', picture: 'https://img.e2e.test/before.png' })
  await serveImages(page)
  await signInWithExtension(page, nsec)
  await expect(avatarImage(page)).toHaveAttribute('src', 'https://img.e2e.test/before.png')

  await page.goto('/profile')
  await expect(page.getByLabel('Name', { exact: true })).toHaveValue('before')
  await page.getByLabel('Picture address').fill('https://img.e2e.test/after.png')
  await page.getByLabel('Name', { exact: true }).fill('after')
  // Marks the page, so a reload would be noticed.
  await page.evaluate(() => { (window as unknown as { __noReload: boolean }).__noReload = true })
  await page.getByTestId('profile-save').click()
  await expect(page.getByTestId('profile-saved')).toContainText('Accepted by')

  await expect(avatarImage(page)).toHaveAttribute('src', 'https://img.e2e.test/after.png')
  await expect(page.getByTestId('user-name')).toHaveText('after')
  expect(await page.evaluate(() => (window as unknown as { __noReload?: boolean }).__noReload)).toBe(true)
})
