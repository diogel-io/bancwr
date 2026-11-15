// Approval before signing (#102) through the real stack: an administrator issues a token whose
// notes need approval, a real NIP-46 client (nostr-tools' BunkerSigner) asks to sign one and is
// pointed at the console, the request waits on the Approvals page, and approving it hands the client
// the signed note. Another is rejected with a note, which the client is given. The bunker's relay is
// the in-process one; from this process it is 127.0.0.1, from the container e2e-host.
import type { Page } from '@playwright/test'
import { BunkerSigner } from 'nostr-tools/nip46'
import { generateSecretKey } from 'nostr-tools/pure'
import { test, expect } from './fixtures'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`

function within<T>(work: Promise<T>, ms = 15_000): Promise<T> {
  return Promise.race([work, new Promise<T>((_, reject) => setTimeout(() => reject(new Error('timed out')), ms))])
}

/**
 * Why `work` was refused: nostr-tools rejects with the bunker's error string. Called as soon as the
 * request is sent, so its rejection is always handled, however long the decision takes.
 */
function refusal(work: Promise<unknown>): Promise<string> {
  return work.then(() => 'signed', (reason: unknown) => reason instanceof Error ? reason.message : String(reason))
}

/** Issues a token for yourself allowing notes and reactions, with notes needing approval. */
async function issueGated(page: Page, label: string): Promise<string> {
  await page.goto('/connections')
  const form = page.getByTestId('issue-token')
  await form.getByRole('combobox').first().click()
  await page.getByRole('option', { name: /\(you\)/ }).click()
  await form.getByLabel('Label').fill(label)
  await form.locator('#issue-token-approval-1').click()
  await form.getByRole('button', { name: 'Issue token' }).click()
  const uri = (await page.getByTestId('issued-uri').textContent())!.trim()
  await page.getByRole('button', { name: 'Done' }).click()
  await expect(page.locator('[data-token]').filter({ hasText: label }).getByTestId('token-approval')).toHaveText('needs approval: 1 Note')
  return uri
}

const note = (content: string) => ({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [['t', 'e2e']], content })

test('a note that needs approval waits on the Approvals page and is signed once approved', async ({ page }) => {
  const uri = await issueGated(page, 'e2e approval scheduler')
  const url = new URL(uri)
  const authUrls: string[] = []
  const app = BunkerSigner.fromBunker(generateSecretKey(), { pubkey: url.hostname, relays: [relay], secret: url.searchParams.get('secret') }, {
    onauth: (authUrl: string) => {
      authUrls.push(authUrl)
    }
  })
  await within(app.connect())

  // A reaction does not need approval.
  await expect(within(app.signEvent({ kind: 7, created_at: Math.floor(Date.now() / 1000), tags: [], content: '+' }))).resolves.toHaveProperty('sig')

  const signing = app.signEvent(note('e2e: waits for approval'))
  await page.goto('/approvals')
  const card = page.locator('[data-approval]').filter({ hasText: 'e2e: waits for approval' })
  await expect(card).toBeVisible({ timeout: 15_000 })
  await expect(card).toContainText('Sign 1 Note')
  const id = await card.getAttribute('data-approval')
  const eventId = (await card.getByTestId('approval-event-id').textContent())!.trim()
  await expect.poll(() => authUrls).toEqual([`${new URL(page.url()).origin}/approvals/${id}`])

  await card.getByLabel(/Note for request/).fill('Ship it')
  await card.getByTestId('approval-approve').click()
  const signed = await within(signing)
  expect(signed.id).toBe(eventId)
  expect(signed.content).toBe('e2e: waits for approval')
  await expect(page.getByTestId('approval-decided')).toContainText('Approved')

  const logs = await (await page.request.get('/api/bunker/logs')).json() as { event_id: string }[]
  expect(logs.map(l => l.event_id)).toContain(eventId)
  // Ended, so later specs that count connected apps see only their own.
  await within(app.logout())
  await app.close()
})

test('a rejected note is not signed, and the app is given the note (#102)', async ({ page }) => {
  const uri = await issueGated(page, 'e2e approval rejected')
  const url = new URL(uri)
  const app = BunkerSigner.fromBunker(generateSecretKey(), { pubkey: url.hostname, relays: [relay], secret: url.searchParams.get('secret') }, { onauth: () => {} })
  await within(app.connect())

  const outcome = refusal(app.signEvent(note('e2e: off brand')))
  await page.goto('/approvals')
  const card = page.locator('[data-approval]').filter({ hasText: 'e2e: off brand' })
  await expect(card).toBeVisible({ timeout: 15_000 })
  await card.getByLabel(/Note for request/).fill('Not on brand')
  await card.getByTestId('approval-reject').click()
  expect(await within(outcome)).toBe('Rejected by an administrator: Not on brand')
  await expect(card).toHaveCount(0)
  // Ended, so later specs that count connected apps see only their own.
  await within(app.logout())
  await app.close()
})
