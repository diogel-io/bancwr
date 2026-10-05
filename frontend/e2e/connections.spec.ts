// Connected apps (#31) through the real stack, with NIP-46 on (compose.e2e.yaml): an administrator
// issues a token on the page, a real NIP-46 client (nostr-tools' BunkerSigner, the frontend's own
// library) connects with it and signs a granted kind, the user sees the app and revokes it, and the
// app can sign no more. The bunker's relay is the in-process one; from this process it is
// 127.0.0.1, from the container e2e-host.
import type { Browser, Page } from '@playwright/test'
import { BunkerSigner } from 'nostr-tools/nip46'
import { generateSecretKey } from 'nostr-tools/pure'
import { contextPost, test, expect } from './fixtures'
import { signInAs } from './sign-in'
import { baseUrl } from './stack'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`

/** Within `ms`, or rejected: a refused request should not leave a test waiting. */
function within<T>(work: Promise<T>, ms = 15_000): Promise<T> {
  return Promise.race([work, new Promise<T>((_, reject) => setTimeout(() => reject(new Error('timed out')), ms))])
}

/** Issues a token on the page for `forName`; returns the bunker:// string shown once. */
async function issueOnPage(page: Page, forName: string | RegExp, label: string): Promise<string> {
  const form = page.getByTestId('issue-token')
  await form.getByRole('combobox').first().click()
  await page.getByRole('option', { name: forName }).click()
  await form.getByLabel('Label').fill(label)
  await form.getByRole('button', { name: 'Issue token' }).click()
  const uri = await page.getByTestId('issued-uri').textContent()
  await expect(page.getByTestId('issued-qr')).toBeVisible()
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
  await expect(page.getByTestId('connections-list')).toContainText('last signed')

  // The signer sees only theirs, and revokes it.
  const user = await asSigner(browser)
  await user.goto('/connections')
  await expect(user.getByTestId('issue-token')).toHaveCount(0)
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

test.describe('viewer', () => {
  test.use({ role: 'viewer' })

  test('cannot open connected apps', async ({ page }) => {
    const response = await page.goto('/connections')
    expect(response?.status()).toBe(403)
    await expect(page.getByRole('heading', { name: /You don't have access to connected apps/ })).toBeVisible()
  })
})
