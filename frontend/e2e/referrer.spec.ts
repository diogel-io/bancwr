// Third-party hosts aren't told which Bancwr is calling (#75), through the real stack: every
// response carries Referrer-Policy: same-origin, and no request to an image, NIP-05 or follows host
// carries a Referer. Each test registers a fresh key, so other specs' profiles are left alone.
import type { Page, Route } from '@playwright/test'
import { SimplePool } from 'nostr-tools/pool'
import { finalizeEvent, generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'
import { continueAsKey, installExtension } from './extension'
import { anonymousTest, expect } from './fixtures'
import { newMember } from './members'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`
const pool = new SimplePool()
const CORS = { 'Access-Control-Allow-Origin': '*' }
// A 1x1 PNG, enough for the browser to load it.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64')

const keyOf = (nsec: string) => decode(nsec).data as Uint8Array

async function publish(secret: Uint8Array, kind: number, tags: string[][], content: string) {
  const event = finalizeEvent({ kind, created_at: Math.floor(Date.now() / 1000) - 60, tags, content }, secret)
  await Promise.any(pool.publish([relay], event))
}

/** Answers every request to `pattern`, recording each request's Referer ('' for none). */
async function recordReferrers(page: Page, pattern: string, answer: (route: Route) => Promise<void>): Promise<{ url: string, referer: string }[]> {
  const seen: { url: string, referer: string }[] = []
  await page.route(pattern, async (route) => {
    seen.push({ url: route.request().url(), referer: (await route.request().allHeaders()).referer ?? '' })
    await answer(route)
  })
  return seen
}

const servePng = (route: Route) => route.fulfill({ body: PNG, contentType: 'image/png' })

async function signIn(page: Page, nsec: string) {
  await installExtension(page, nsec)
  await page.goto('/sign-in')
  await page.getByRole('button', { name: 'Sign in with extension' }).click()
  await continueAsKey(page)
  await expect(page).toHaveURL('/')
}

anonymousTest('every response says Referrer-Policy: same-origin, header and meta', async ({ page }) => {
  const response = await page.goto('/sign-in')
  expect(response!.headers()['referrer-policy']).toBe('same-origin')
  await expect(page.locator('meta[name="referrer"]')).toHaveAttribute('content', 'same-origin')
  const api = await page.request.get('/api/version')
  expect(api.headers()['referrer-policy']).toBe('same-origin')
})

anonymousTest('the profile page\'s pictures and the NIP-05 lookup send no Referer', async ({ page }) => {
  const nsec = await newMember()
  const pubkey = getPublicKey(keyOf(nsec))
  await publish(keyOf(nsec), 0, [], JSON.stringify({
    name: 'referrer', picture: 'https://img.e2e.test/picture.png', banner: 'https://img.e2e.test/banner.png'
  }))
  const images = await recordReferrers(page, 'https://img.e2e.test/**', servePng)
  const nip05 = await recordReferrers(page, 'https://nip05.e2e.test/.well-known/nostr.json*', route =>
    route.fulfill({ json: { names: { referrer: pubkey } }, headers: CORS }))
  await signIn(page, nsec)

  await page.goto('/profile')
  await expect(page.getByLabel('Name', { exact: true })).toHaveValue('referrer')
  await expect(page.getByTestId('profile-preview').locator('img[src$="banner.png"]')).toBeVisible()
  await expect(page.getByTestId('profile-preview').locator('img[src$="picture.png"]')).toBeVisible()
  await page.getByLabel('NIP-05 identifier').fill('referrer@nip05.e2e.test')
  await page.getByRole('button', { name: 'Verify identifier' }).click()
  await expect(page.getByTestId('nip05-status')).toContainText('returned this account\'s public key')

  expect(images.map(seen => seen.url)).toEqual(expect.arrayContaining(['https://img.e2e.test/picture.png', 'https://img.e2e.test/banner.png']))
  expect(images.filter(seen => seen.referer)).toEqual([])
  expect(nip05.length).toBeGreaterThan(0)
  expect(nip05.filter(seen => seen.referer)).toEqual([])
})

anonymousTest('the follows page\'s avatars send no Referer', async ({ page }) => {
  const nsec = await newMember()
  const friend = generateSecretKey()
  await publish(friend, 0, [], JSON.stringify({ display_name: 'Friend', picture: 'https://img.e2e.test/friend.png' }))
  await publish(keyOf(nsec), 3, [['p', getPublicKey(friend)]], '')
  const images = await recordReferrers(page, 'https://img.e2e.test/**', servePng)
  await signIn(page, nsec)

  await page.goto('/follows')
  await expect(page.getByTestId('follows-list')).toContainText('Friend')
  await expect(page.getByTestId('follows-list').locator('img[src$="friend.png"]')).toBeVisible()

  expect(images.map(seen => seen.url)).toContain('https://img.e2e.test/friend.png')
  expect(images.filter(seen => seen.referer)).toEqual([])
})
