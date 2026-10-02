// The profile page (#30), through the real stack: the member's own signer signs, and the update
// lands on the relay with every field another app set kept. The in-process relay (relay.ts) is the
// profile relay here (compose.e2e.yaml); NIP-05 and Blossom hosts are answered with page.route.
import { createHash } from 'node:crypto'
import { crc32, deflateSync } from 'node:zlib'
import type { Page, Route } from '@playwright/test'
import { SimplePool } from 'nostr-tools/pool'
import { finalizeEvent, getPublicKey, type NostrEvent } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'
import { installExtension } from './extension'
import { anonymousTest, test, expect, type Role } from './fixtures'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`
const indexer = `ws://127.0.0.1:${process.env.E2E_INDEXER_PORT || 7778}`
const pool = new SimplePool()

const keyOf = (nsec: string) => decode(nsec).data as Uint8Array

async function seedProfile(nsec: string, content: Record<string, unknown>, on = relay) {
  const event = finalizeEvent({ kind: 0, created_at: Math.floor(Date.now() / 1000) - 60, tags: [], content: JSON.stringify(content) }, keyOf(nsec))
  await Promise.any(pool.publish([on], event))
}

async function newestProfile(pubkey: string): Promise<NostrEvent | undefined> {
  const events = await pool.querySync([relay], { kinds: [0], authors: [pubkey] })
  return events.sort((a, b) => b.created_at - a.created_at)[0]
}

async function signInWithExtension(page: Page, nsec: string) {
  await installExtension(page, nsec)
  await page.goto('/sign-in')
  await page.getByRole('button', { name: 'Sign in with extension' }).click()
  await expect(page).toHaveURL('/')
}

const nameField = (page: Page) => page.getByLabel('Display name', { exact: true })
const CORS = { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Methods': 'GET, PUT, OPTIONS', 'Access-Control-Allow-Headers': 'Authorization, Content-Type' }

anonymousTest('a member signed in with NIP-07 edits their profile, and fields set elsewhere survive', async ({ page }) => {
  const nsec = process.env.E2E_USER_NSEC!
  const pubkey = getPublicKey(keyOf(nsec))
  await seedProfile(nsec, { name: 'user', display_name: 'Before', pronouns: 'they/them' })
  await signInWithExtension(page, nsec)

  await page.goto('/profile')
  await expect(nameField(page)).toHaveValue('Before')
  await nameField(page).fill('After')
  await page.getByTestId('profile-save').click()
  await expect(page.getByTestId('profile-saved')).toContainText('Accepted by')

  const saved = await newestProfile(pubkey)
  expect(JSON.parse(saved!.content)).toEqual({ name: 'user', display_name: 'After', pronouns: 'they/them' })
  expect(saved!.pubkey).toBe(pubkey)
})

anonymousTest('a member signed in with NIP-46 saves through the same remote signer, after a reload', async ({ page }) => {
  const pubkey = new URL(process.env.E2E_NIP46_URI!).hostname
  await page.goto('/sign-in')
  await page.getByPlaceholder('bunker://…').fill(process.env.E2E_NIP46_URI!)
  await page.getByRole('button', { name: 'Connect and sign in' }).click()
  await expect(page).toHaveURL('/', { timeout: 30_000 })

  // A full load: the connection is resumed from this tab's session, not made again.
  await page.goto('/profile')
  await expect(page.getByRole('heading', { name: 'Profile Details' })).toBeVisible()
  await page.getByLabel('Name', { exact: true }).fill('remote-signed')
  // This key has no profile anywhere, so creating one is confirmed first (#62).
  await page.getByRole('checkbox').check()
  await page.getByTestId('profile-save').click()
  await expect(page.getByTestId('profile-saved')).toBeVisible({ timeout: 30_000 })

  expect(JSON.parse((await newestProfile(pubkey))!.content).name).toBe('remote-signed')
})

anonymousTest('NIP-05 is verified only on request, and a mismatch is reported', async ({ page }) => {
  const nsec = process.env.E2E_USER_NSEC!
  const pubkey = getPublicKey(keyOf(nsec))
  const asked: string[] = []
  await page.route('https://nip05.e2e.test/.well-known/nostr.json*', async (route) => {
    asked.push(route.request().url())
    await route.fulfill({ json: { names: { alice: pubkey, mallory: 'cd'.repeat(32) } }, headers: CORS })
  })
  await signInWithExtension(page, nsec)
  await page.goto('/profile')

  const identifier = page.getByLabel('NIP-05 identifier')
  const status = page.getByTestId('nip05-status')
  await identifier.fill('alice@nip05.e2e.test')
  expect(asked).toEqual([])

  await page.getByRole('button', { name: 'Verify identifier' }).click()
  await expect(status).toContainText('nip05.e2e.test returned this account\'s public key.')

  await identifier.fill('mallory@nip05.e2e.test')
  await expect(status).toHaveAttribute('data-tone', 'neutral')
  await page.getByRole('button', { name: 'Verify identifier' }).click()
  await expect(status).toContainText('This identifier resolves to a different public key.')
})

anonymousTest('a picture is uploaded to Blossom, authorised by the member\'s own key', async ({ page }) => {
  const nsec = process.env.E2E_USER_NSEC!
  const pubkey = getPublicKey(keyOf(nsec))
  const stored = new Map<string, Buffer>()
  let authorisedBy: string | undefined
  await page.route('https://blossom.e2e.test/**', async (route: Route) => {
    const request = route.request()
    if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: CORS })
    if (request.method() === 'PUT') {
      const body = request.postDataBuffer()!
      const sha256 = createHash('sha256').update(body).digest('hex')
      const auth = JSON.parse(Buffer.from(request.headers().authorization!.slice(6), 'base64').toString()) as NostrEvent
      if (auth.kind !== 24242 || !auth.tags.some(t => t[0] === 'x' && t[1] === sha256)) return route.fulfill({ status: 401, headers: CORS })
      authorisedBy = auth.pubkey
      stored.set(sha256, body)
      return route.fulfill({ json: { url: `https://blossom.e2e.test/${sha256}.webp`, sha256, size: body.length, type: 'image/webp' }, headers: CORS })
    }
    const blob = stored.get(new URL(request.url()).pathname.slice(1).replace(/\.webp$/, ''))
    return blob ? route.fulfill({ body: blob, contentType: 'image/webp', headers: CORS }) : route.fulfill({ status: 404, headers: CORS })
  })
  await signInWithExtension(page, nsec)
  await page.goto('/profile')

  const png = solidPng(64, 48)
  await page.getByTestId('picture-file').setInputFiles({ name: 'me.png', mimeType: 'image/png', buffer: png })

  await expect(page.getByLabel('Picture address')).toHaveValue(/^https:\/\/blossom\.e2e\.test\/[0-9a-f]{64}\.webp$/)
  expect(authorisedBy).toBe(pubkey)
  // Re-encoded, so not the bytes chosen (no metadata carried over).
  expect([...stored.values()][0]!.equals(png)).toBe(false)
})

anonymousTest('a profile and relay list held only by an indexer are found (#62)', async ({ page }) => {
  // #62 as reported: the key's NIP-65 list names a relay that is down, and the list and profile are
  // on none of the default relays, only on an indexer.
  const nsec = process.env.E2E_SIGNER_NSEC!
  const key = keyOf(nsec)
  const list = finalizeEvent({ kind: 10002, created_at: Math.floor(Date.now() / 1000) - 120, tags: [['r', 'ws://127.0.0.1:1']], content: '' }, key)
  await Promise.any(pool.publish([indexer], list))
  await seedProfile(nsec, { name: 'indexed', display_name: 'Found on the indexer' }, indexer)
  expect(await pool.querySync([relay], { kinds: [0, 10002], authors: [getPublicKey(key)] })).toEqual([])

  await signInWithExtension(page, nsec)
  await page.goto('/profile')
  await expect(nameField(page)).toHaveValue('Found on the indexer')
  await expect(page.getByTestId('profile-not-found')).toHaveCount(0)
})

test('a key with no profile anywhere is told where was searched, and must confirm creating one', async ({ page }) => {
  await page.goto('/profile')
  await expect(page.getByTestId('profile-not-found')).toBeVisible()
  await expect(page.getByTestId('searched-relays')).toContainText('127.0.0.1:7778')
  await page.getByLabel('Name', { exact: true }).fill('new')
  const create = page.getByTestId('profile-save')
  await expect(create).toHaveText('Create profile')
  await expect(create).toBeDisabled()
  await page.getByRole('checkbox').check()
  await expect(create).toBeEnabled()
})

for (const role of ['administrator', 'user', 'signer'] as Role[]) {
  test.describe(role, () => {
    test.use({ role })

    test('can open their own profile', async ({ page }) => {
      const response = await page.goto('/profile')
      expect(response?.status()).toBe(200)
      await expect(page.getByRole('heading', { name: 'Profile Details' })).toBeVisible()
      await expect(page.getByRole('navigation').getByRole('link', { name: 'Profile', exact: true })).toBeVisible()
    })
  })
}

test('is a single column on a phone', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 })
  await page.goto('/profile')
  await expect(page.getByRole('heading', { name: 'Profile Details' })).toBeVisible()
  const form = (await page.getByRole('heading', { name: 'Profile Details' }).boundingBox())!
  const preview = (await page.getByTestId('profile-preview').boundingBox())!
  expect(preview.y).toBeGreaterThan(form.y)
  expect(Math.abs(preview.x - form.x)).toBeLessThan(40)
})

/** A valid PNG of one colour, so the page has a real image to decode and re-encode. */
function solidPng(width: number, height: number): Buffer {
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type), data])
    const length = Buffer.alloc(4)
    length.writeUInt32BE(data.length)
    const crc = Buffer.alloc(4)
    crc.writeUInt32BE(crc32(body))
    return Buffer.concat([length, body, crc])
  }
  const header = Buffer.alloc(13)
  header.writeUInt32BE(width, 0)
  header.writeUInt32BE(height, 4)
  header.writeUInt8(8, 8) // bit depth
  header.writeUInt8(2, 9) // RGB
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(width * 3, 0x80)])
  const pixels = deflateSync(Buffer.concat(Array.from({ length: height }, () => row)))
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', header), chunk('IDAT', pixels), chunk('IEND', Buffer.alloc(0))])
}
