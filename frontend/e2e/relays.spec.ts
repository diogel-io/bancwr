// The relay list page (#33), through the real stack: marker changes, a removal and an add, signed
// by the member's own key, published to the profile relay and the indexer alike, and every tag the
// change did not touch kept. The in-process relays stand in for the member's relays.
import { SimplePool } from 'nostr-tools/pool'
import { finalizeEvent, getPublicKey, type NostrEvent } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'
import { continueAsKey, installExtension } from './extension'
import { anonymousTest, test, expect } from './fixtures'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`
const indexer = `ws://127.0.0.1:${process.env.E2E_INDEXER_PORT || 7778}`
const pool = new SimplePool()

async function newestList(on: string, pubkey: string): Promise<NostrEvent | undefined> {
  const events = await pool.querySync([on], { kinds: [10002], authors: [pubkey] })
  return events.sort((a, b) => b.created_at - a.created_at)[0]
}

anonymousTest('a member edits their relay list, and it reaches every relay with untouched tags kept', async ({ page }) => {
  const nsec = process.env.E2E_USER_NSEC!
  const key = decode(nsec).data as Uint8Array
  const me = getPublicKey(key)
  const seeded = finalizeEvent({
    kind: 10002,
    created_at: Math.floor(Date.now() / 1000) - 60,
    tags: [['r', relay], ['r', 'wss://read-only.example', 'read'], ['alt', 'relay list'], ['r', 'wss://write-only.example', 'write']],
    content: ''
  }, key)
  await Promise.any(pool.publish([relay], seeded))

  await installExtension(page, nsec)
  await page.goto('/sign-in')
  await page.getByRole('button', { name: 'Sign in with extension' }).click()
  await continueAsKey(page)
  await expect(page).toHaveURL('/')
  await page.getByRole('navigation').getByRole('link', { name: 'Relays', exact: true }).click()

  const row = (url: string) => page.locator(`[data-relay="${url}"]`)
  await expect(row(relay)).toBeVisible()
  await expect(row('wss://read-only.example').getByRole('checkbox', { name: 'Write' })).not.toBeChecked()

  // The local relay becomes write-only, read-only.example goes, the indexer is added.
  await row(relay).getByRole('checkbox', { name: 'Read' }).uncheck()
  await row('wss://read-only.example').getByRole('button', { name: /^Remove/ }).click()
  await page.getByLabel('Add a relay').fill('https://not-a-relay.example')
  await page.getByTestId('add-relay').getByRole('button', { name: 'Add' }).click()
  await expect(page.getByTestId('add-relay')).toContainText('Relay addresses start with wss://')
  await page.getByLabel('Add a relay').fill(indexer)
  await page.getByTestId('add-relay').getByRole('button', { name: 'Add' }).click()

  await page.getByTestId('relays-save').click()
  await expect(page.getByTestId('relays-saved')).toContainText('Accepted by')

  await page.reload()
  await expect(row(relay).getByRole('checkbox', { name: 'Read' })).not.toBeChecked()
  await expect(row('wss://read-only.example')).toHaveCount(0)
  await expect(row(indexer)).toBeVisible()

  const expected = [['r', relay, 'write'], ['alt', 'relay list'], ['r', 'wss://write-only.example', 'write'], ['r', indexer]]
  expect((await newestList(relay, me))!.tags).toEqual(expected)
  expect((await newestList(indexer, me))!.tags).toEqual(expected)
})

test.describe('signer', () => {
  test.use({ role: 'signer' })

  test('cannot open the relay list', async ({ page }) => {
    const response = await page.goto('/relays')
    expect(response?.status()).toBe(403)
    await expect(page.getByRole('heading', { name: /You don't have access to default relays/ })).toBeVisible()
  })
})
