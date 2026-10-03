// The follow list page (#32), through the real stack: changes signed by the member's own key, and
// the published kind 3 keeping every tag, petname, relay hint and the content it did not change.
// The in-process relays are the profile relay and the indexer (compose.e2e.yaml).
import type { Page } from '@playwright/test'
import { SimplePool } from 'nostr-tools/pool'
import { finalizeEvent, generateSecretKey, getPublicKey, type NostrEvent } from 'nostr-tools/pure'
import { decode, npubEncode } from 'nostr-tools/nip19'
import { continueAsKey, installExtension } from './extension'
import { anonymousTest, test, expect } from './fixtures'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`
const indexer = `ws://127.0.0.1:${process.env.E2E_INDEXER_PORT || 7778}`
const pool = new SimplePool()
const CORS = { 'Access-Control-Allow-Origin': '*' }

const keyOf = (nsec: string) => decode(nsec).data as Uint8Array
const someone = () => getPublicKey(generateSecretKey())

async function publish(on: string, secret: Uint8Array, kind: number, tags: string[][], content = '') {
  const event = finalizeEvent({ kind, created_at: Math.floor(Date.now() / 1000) - 60, tags, content }, secret)
  await Promise.any(pool.publish([on], event))
}

async function newestList(pubkey: string): Promise<NostrEvent | undefined> {
  const events = await pool.querySync([relay], { kinds: [3], authors: [pubkey] })
  return events.sort((a, b) => b.created_at - a.created_at)[0]
}

async function addFollow(page: Page, value: string) {
  await page.getByLabel('Follow someone').fill(value)
  await page.getByRole('button', { name: 'Find', exact: true }).click()
  await page.getByTestId('add-follow-preview').getByRole('button', { name: 'Add' }).click()
}

anonymousTest('a member edits their follows, and everything they did not change survives', async ({ page }) => {
  const nsec = process.env.E2E_USER_NSEC!
  const me = getPublicKey(keyOf(nsec))
  const alice = generateSecretKey()
  const [bob, carol, dave] = [someone(), someone(), someone()]
  await publish(relay, alice, 0, [], JSON.stringify({ display_name: 'Alice' }))
  await publish(relay, keyOf(nsec), 3, [['p', getPublicKey(alice), 'wss://alice.example/', 'ally'], ['t', 'nostr'], ['p', bob]], '{"wss://old.example/":{"read":true}}')
  await page.route('https://nip05.e2e.test/.well-known/nostr.json*', route => route.fulfill({ json: { names: { dave } }, headers: CORS }))

  await installExtension(page, nsec)
  await page.goto('/sign-in')
  await page.getByRole('button', { name: 'Sign in with extension' }).click()
  await continueAsKey(page)
  await expect(page).toHaveURL('/')
  await page.getByRole('navigation').getByRole('link', { name: 'Follows', exact: true }).click()

  const list = page.getByTestId('follows-list')
  await expect(list).toContainText('Alice')
  await expect(page.getByTestId('follows-count')).toHaveText('2')

  await list.locator(`[data-pubkey="${bob}"]`).getByRole('button').click()
  await addFollow(page, npubEncode(carol))
  await addFollow(page, 'dave@nip05.e2e.test')
  await expect(page.getByTestId('follows-pending')).toHaveText('2 to follow, 1 to unfollow')
  await page.getByTestId('follows-save').click()
  await expect(page.getByTestId('follows-saved')).toContainText('Accepted by')

  await page.reload()
  await expect(page.getByTestId('follows-count')).toHaveText('3')
  await expect(list.locator(`[data-pubkey="${bob}"]`)).toHaveCount(0)

  const saved = await newestList(me)
  expect(saved!.tags).toEqual([['p', getPublicKey(alice), 'wss://alice.example/', 'ally'], ['t', 'nostr'], ['p', carol], ['p', dave]])
  expect(saved!.content).toBe('{"wss://old.example/":{"read":true}}')
})

test('a follow list held only by an indexer is found (#62)', async ({ page }) => {
  const admin = keyOf(process.env.E2E_ADMIN_NSEC!)
  await publish(indexer, admin, 3, [['p', someone()], ['p', someone()], ['p', someone()]])
  expect(await pool.querySync([relay], { kinds: [3], authors: [getPublicKey(admin)] })).toEqual([])

  await page.goto('/follows')
  await expect(page.getByTestId('follows-count')).toHaveText('3')
  await expect(page.getByTestId('follows-not-found')).toHaveCount(0)
})

test.describe('signer', () => {
  test.use({ role: 'signer' })

  test('cannot open the follow list', async ({ page }) => {
    const response = await page.goto('/follows')
    expect(response?.status()).toBe(403)
    await expect(page.getByRole('heading', { name: /You don't have access to follows/ })).toBeVisible()
  })
})
