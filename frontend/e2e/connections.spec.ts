// Connected apps (#31) through the real stack, with NIP-46 on (compose.e2e.yaml): an administrator
// issues a token on the page, a real NIP-46 client (nostr-tools' BunkerSigner, the frontend's own
// library) connects with it and signs a granted kind, the user sees the app and revokes it, and the
// app can sign no more. An app granted the encryption methods (#97) encrypts and decrypts NIP-44
// with the brand's key, and each decrypt is in the export. A connection issued for 7 days (#98)
// shows when it ends. A connection issued with a rate limit (#101) is refused past its burst. A
// signer issues their own token on the page and connects an app with it (#142). The
// bunker's relay is the in-process
// one; from this process it is 127.0.0.1, from the container e2e-host.
import type { Browser, Page } from '@playwright/test'
import { BunkerSigner } from 'nostr-tools/nip46'
import * as nip44 from 'nostr-tools/nip44'
import { generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { contextPost, test, expect } from './fixtures'
import { signInAs } from './sign-in'
import { baseUrl } from './stack'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`

/** Within `ms`, or rejected: a refused request should not leave a test waiting. */
function within<T>(work: Promise<T>, ms = 15_000): Promise<T> {
  return Promise.race([work, new Promise<T>((_, reject) => setTimeout(() => reject(new Error('timed out')), ms))])
}

/**
 * Issues a token on the page for `forName`; returns the bunker:// string shown once. The form and
 * the result name the primary identity as the key the app signs as, whoever it is for (#137).
 */
async function issueOnPage(page: Page, forName: string | RegExp, label: string): Promise<string> {
  const primary = process.env.E2E_BUNKER_NPUB
  const form = page.getByTestId('issue-token')
  await form.getByRole('combobox').first().click()
  await page.getByRole('option', { name: forName }).click()
  if (primary) await expect(form.getByTestId('issue-token-signs-as')).toContainText(primary)
  await form.getByLabel('Label').fill(label)
  await form.getByRole('button', { name: 'Issue token' }).click()
  const uri = await page.getByTestId('issued-uri').textContent()
  await expect(page.getByTestId('issued-qr')).toBeVisible()
  if (primary) await expect(page.getByTestId('issued-identity')).toContainText(primary)
  await page.getByRole('button', { name: 'Done' }).click()
  return uri!.trim()
}

/** Why `work` was refused: nostr-tools rejects with the bunker's error string, not an Error. */
async function refusal(work: Promise<unknown>): Promise<string> {
  return within(work).then(() => 'signed', (reason: unknown) => reason instanceof Error ? reason.message : String(reason))
}

/** A NIP-46 client connected with `uri`, reaching the bunker through the relay as this process sees it. */
async function connectApp(uri: string): Promise<BunkerSigner> {
  const url = new URL(uri)
  const signer = BunkerSigner.fromBunker(generateSecretKey(), { pubkey: url.hostname, relays: [relay], secret: url.searchParams.get('secret') })
  await within(signer.connect())
  return signer
}

async function asSigner(browser: Browser): Promise<Page> {
  const context = await browser.newContext({ baseURL: baseUrl() })
  const page = await context.newPage()
  const login = await signInAs(process.env.E2E_SIGNER_NSEC!, baseUrl(), contextPost(page.request))
  expect(login.status).toBe(200)
  return page
}

const note = () => ({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [], content: 'from a connected app' })

test('a signer connects an app for themselves, which signs only what they granted (#142)', async ({ browser }) => {
  const user = await asSigner(browser)
  await user.goto('/connections')
  const form = user.getByTestId('issue-token')
  await expect(form.getByTestId('issue-token-approval')).toHaveCount(0)
  await form.getByLabel('Label').fill('e2e app the signer connected')
  await form.getByRole('button', { name: 'Issue token' }).click()
  const uri = (await user.getByTestId('issued-uri').textContent())!.trim()
  expect(uri).toMatch(/^bunker:\/\/[0-9a-f]{64}\?relay=.+&secret=[0-9a-f]{64}$/)
  await user.getByRole('button', { name: 'Done' }).click()

  const app = await connectApp(uri)
  await expect(within(app.signEvent(note()))).resolves.toHaveProperty('sig')
  expect(await refusal(app.signEvent({ ...note(), kind: 0, content: '{}' }))).toContain('Forbidden')

  await user.reload()
  const mine = user.getByTestId('connections-list').locator('li')
  await expect(mine.first()).toContainText('1 Note')
  await mine.first().getByRole('button', { name: /^Revoke/ }).click()
  await mine.first().getByRole('button', { name: 'Revoke', exact: true }).click()
  expect(await refusal(app.signEvent(note()))).toContain('Not connected')
  await app.close()
})

test('an administrator connects an app for a signer, who sees it and revokes it', async ({ page, browser }) => {
  await page.goto('/connections')
  await expect(page.getByTestId('connections-explainer')).toContainText('not your own')

  const uri = await issueOnPage(page, 'e2e signer', 'e2e app for the signer')
  expect(uri).toMatch(/^bunker:\/\/[0-9a-f]{64}\?relay=.+&secret=[0-9a-f]{64}$/)
  // Shown once: gone after Done.
  await expect(page.getByTestId('issued-uri')).toHaveCount(0)

  const app = await connectApp(uri)
  const signed = await within(app.signEvent(note()))
  expect(signed.pubkey).toBe(new URL(uri).hostname)
  expect(await refusal(app.signEvent({ ...note(), kind: 0, content: '{}' }))).toContain('Forbidden')

  // Another app, for the administrator, which the signer must not see.
  // The seeded administrator is listed as "Administrator (bootstrap) (you)".
  const own = await connectApp(await issueOnPage(page, /\(you\)/, 'e2e app for the admin'))

  await page.reload()
  const rows = page.getByTestId('connections-list').locator('li')
  await expect(rows).toHaveCount(2)
  await expect(page.getByTestId('connections-list')).toContainText('for e2e signer')
  await expect(page.getByTestId('connections-list')).toContainText('last used')

  // The signer sees only theirs, and revokes it.
  const user = await asSigner(browser)
  await user.goto('/connections')
  // A signer's own form issues only for them (#142): no member to choose.
  await expect(user.getByTestId('issue-token')).toBeVisible()
  await expect(user.locator('#issue-token-for')).toHaveCount(0)
  const mine = user.getByTestId('connections-list').locator('li')
  await expect(mine).toHaveCount(1)
  await expect(mine.first()).toContainText('1 Note')
  await mine.first().getByRole('button', { name: /^Revoke/ }).click()
  await mine.first().getByRole('button', { name: 'Revoke', exact: true }).click()
  await expect(user.getByTestId('connections-empty')).toContainText('No apps are connected for you')

  // The revoked app can sign no more; the administrator's still can.
  expect(await refusal(app.signEvent(note()))).toContain('Not connected')
  await expect(within(own.signEvent(note()))).resolves.toHaveProperty('sig')
  await app.close()
  await own.close()
  await user.context().close()
})

test('an app granted NIP-44 encrypts and decrypts as the brand, and each decrypt is recorded (#97)', async ({ page }) => {
  await page.goto('/connections')
  const form = page.getByTestId('issue-token')
  await form.getByRole('combobox').first().click()
  await page.getByRole('option', { name: /\(you\)/ }).click()
  await form.getByLabel('Label').fill('e2e messages app')
  await form.locator('#issue-token-kinds').click()
  await page.getByRole('option', { name: 'Nothing: encryption only' }).click()
  await expect(form.getByRole('button', { name: 'Issue token' })).toBeDisabled()
  await form.getByLabel('Encrypt messages (NIP-44)').check()
  await form.getByLabel('Read encrypted messages (NIP-44)').check()
  await expect(page.getByTestId('issue-token-decrypt-warning')).toContainText('private messages')
  await form.getByRole('button', { name: 'Issue token' }).click()
  const uri = (await page.getByTestId('issued-uri').textContent())!.trim()
  await page.getByRole('button', { name: 'Done' }).click()

  const app = await connectApp(uri)
  const brand = new URL(uri).hostname
  const friend = generateSecretKey()
  const friendPubkey = getPublicKey(friend)

  // What the app encrypts as the brand, the friend reads; what the friend sends, the app reads.
  const sent = await within(app.nip44Encrypt(friendPubkey, 'hello from the brand'))
  expect(nip44.decrypt(sent, nip44.getConversationKey(friend, brand))).toBe('hello from the brand')
  const incoming = nip44.encrypt('hello brand', nip44.getConversationKey(friend, brand))
  expect(await within(app.nip44Decrypt(friendPubkey, incoming))).toBe('hello brand')
  // Nothing else was granted.
  expect(await refusal(app.signEvent(note()))).toContain('Forbidden')
  await app.close()

  await page.reload()
  // The only app here granted any method.
  await expect(page.getByTestId('connection-methods')).toHaveCount(1)
  await expect(page.getByTestId('connection-methods')).toContainText('Read encrypted messages (NIP-44)')

  // The decrypt is in the export, with its peer and without the message.
  const today = new Date().toISOString().slice(0, 10)
  const csv = await (await page.request.get(`/api/bunker/logs/export?format=csv&from=${today}&to=${today}`)).text()
  // The identity column is last (#103), empty for the primary.
  const decrypts = csv.split('\r\n').filter(l => l.includes(',audit,nip44_decrypt,') && l.endsWith(`peer=${friendPubkey},`))
  expect(decrypts).toHaveLength(1)
  expect(csv).not.toContain('hello brand')
  expect(csv).not.toContain(incoming)
})

test('a connection issued for 7 days signs and shows when it ends (#98)', async ({ page }) => {
  await page.goto('/connections')
  const form = page.getByTestId('issue-token')
  await form.getByRole('combobox').first().click()
  await page.getByRole('option', { name: /\(you\)/ }).click()
  await form.getByLabel('Label').fill('e2e contractor app')
  await form.locator('#issue-token-lifetime').click()
  await page.getByRole('option', { name: '7 days' }).click()
  await form.getByRole('button', { name: 'Issue token' }).click()
  const uri = (await page.getByTestId('issued-uri').textContent())!.trim()
  await page.getByRole('button', { name: 'Done' }).click()

  const app = await connectApp(uri)
  await expect(within(app.signEvent(note()))).resolves.toHaveProperty('sig')
  await app.close()

  await page.reload()
  await expect(page.getByTestId('connection-end').filter({ hasText: 'ends in 7 days' })).toHaveCount(1)
})

test('a connection issued with a rate limit is refused past its burst and shows its limit (#101)', async ({ page }) => {
  await page.goto('/connections')
  const form = page.getByTestId('issue-token')
  await form.getByRole('combobox').first().click()
  await page.getByRole('option', { name: /\(you\)/ }).click()
  await form.getByLabel('Label').fill('e2e rate-limited bot')
  await form.locator('#issue-token-rate-limit').click()
  await page.getByRole('option', { name: '10 a minute (burst 5)' }).click()
  await form.getByRole('button', { name: 'Issue token' }).click()
  const uri = (await page.getByTestId('issued-uri').textContent())!.trim()
  await page.getByRole('button', { name: 'Done' }).click()

  const token = page.locator('[data-token]').filter({ hasText: 'e2e rate-limited bot' })
  await expect(token.getByTestId('token-rate-limit')).toHaveText('limited to 10 a minute (burst 5)')

  const app = await connectApp(uri)
  for (let n = 0; n < 5; n++) {
    await expect(within(app.signEvent(note()))).resolves.toHaveProperty('sig')
  }
  expect(await refusal(app.signEvent(note()))).toContain('Rate limited: this connection may make 10 requests a minute (burst 5)')
  await app.close()

  await page.reload()
  await expect(page.getByTestId('connection-rate-limit').filter({ hasText: 'limited to 10 a minute (burst 5)' })).toHaveCount(1)
})

test.describe('viewer', () => {
  test.use({ role: 'viewer' })

  test('cannot open connected apps', async ({ page }) => {
    const response = await page.goto('/connections')
    expect(response?.status()).toBe(403)
    await expect(page.getByRole('heading', { name: /You don't have access to connected apps/ })).toBeVisible()
  })
})
