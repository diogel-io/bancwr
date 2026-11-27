// Several identities in one bunker (#103) through the real stack: stack.ts adds a second identity
// with `bunker import-key --add` before start. An administrator sees both on Config, puts the e2e
// signer in that identity's department on their team page (#147), issues a token for it, and a real NIP-46 client
// (nostr-tools' BunkerSigner) signs as that identity, not the primary. A token for an identity the
// member is not granted is refused. The bunker's relay is the in-process one.
import { SimplePool } from 'nostr-tools/pool'
import { BunkerSigner } from 'nostr-tools/nip46'
import { decode, npubEncode } from 'nostr-tools/nip19'
import { generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { test, expect, sameOrigin } from './fixtures'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`

function within<T>(work: Promise<T>, ms = 15_000): Promise<T> {
  return Promise.race([work, new Promise<T>((_, reject) => setTimeout(() => reject(new Error('timed out')), ms))])
}

const hexOf = (npub: string) => decode(npub).data as string

test.skip(!process.env.E2E_BRAND_NPUB, 'needs the stack started here, which adds the second identity')

test('an administrator connects an app that signs as a second identity (#103)', async ({ page }) => {
  const brand = hexOf(process.env.E2E_BRAND_NPUB!)
  const signer = getPublicKey(decode(process.env.E2E_SIGNER_NSEC!).data as Uint8Array)

  // Both identities are on Config.
  await page.goto('/config')
  const section = page.getByTestId('identities-section')
  await expect(section.locator('[data-identity]')).toHaveCount(2)
  await expect(section.locator(`[data-identity="${brand}"]`)).toContainText('e2e brand')

  // The signer was added with the primary only, so Brand cannot be issued for them yet.
  const refused = await page.request.post('/api/bunker/connections/tokens', {
    headers: sameOrigin,
    data: { for_pubkey: signer, label: 'not yet', kinds: [1], identity: brand }
  })
  expect(refused.status()).toBe(400)
  // The proxy passes the bunker's refusal on as an h3 error, its body under `data`.
  const body = await refused.json() as { data?: { error?: string } }
  expect(body.data?.error).toBe('no_identity_access')

  // Put in its department on their team page (#147).
  await page.goto(`/team/${signer}`)
  const access = page.getByTestId('identity-access')
  await expect(access.getByLabel('Role in Primary')).toContainText('Member')
  await access.getByLabel('Role in e2e brand').click()
  await page.getByRole('option', { name: 'Member', exact: true }).click()
  await expect(access.getByLabel('Role in e2e brand')).toContainText('Member')

  // Issued for the signer, signing as the brand.
  await page.goto('/connections')
  const form = page.getByTestId('issue-token')
  await form.locator('#issue-token-for').click()
  await page.getByRole('option', { name: 'e2e signer', exact: true }).click()
  await form.locator('#issue-token-identity').click()
  await page.getByRole('option', { name: 'e2e brand' }).click()
  // The form and the result name the brand's key, not the signer's (#137).
  await expect(form.getByTestId('issue-token-signs-as')).toHaveText(`Signs as e2e brand (${process.env.E2E_BRAND_NPUB})`)
  await form.getByLabel('Label').fill('e2e brand app')
  await form.getByRole('button', { name: 'Issue token' }).click()
  await expect(page.getByTestId('issued-identity')).toHaveText(`Signs as e2e brand (${process.env.E2E_BRAND_NPUB})`)
  const uri = (await page.getByTestId('issued-uri').textContent())!.trim()
  await page.getByRole('button', { name: 'Done' }).click()
  expect(new URL(uri).hostname).toBe(brand)

  const url = new URL(uri)
  const app = BunkerSigner.fromBunker(generateSecretKey(), { pubkey: url.hostname, relays: [relay], secret: url.searchParams.get('secret') })
  await within(app.connect())
  expect(await within(app.getPublicKey())).toBe(brand)
  const signed = await within(app.signEvent({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [], content: 'from the e2e brand' }))
  expect(signed.pubkey).toBe(brand)
  await within(app.logout())
  await app.close()

  // The log names the identity that signed.
  const logs = await (await page.request.get('/api/bunker/logs')).json() as { event_id: string, identity_pubkey?: string }[]
  expect(logs.find(l => l.event_id === signed.id)?.identity_pubkey).toBe(brand)
})

test('an administrator creates an identity in the console, an app signs as it, and retiring it ends the app (#146)', async ({ page }) => {
  const signer = getPublicKey(decode(process.env.E2E_SIGNER_NSEC!).data as Uint8Array)
  const label = `e2e sales ${Date.now()}`

  // Created on Config from a label alone, with no restart.
  await page.goto('/config')
  const section = page.getByTestId('identities-section')
  await section.locator('#identity-create-label').fill(label)
  await section.getByRole('button', { name: 'Create identity' }).click()
  await expect(section.getByTestId('identity-created')).toContainText(`Created ${label}`)
  const row = section.locator('[data-identity]', { hasText: label })
  await expect(row).toBeVisible()
  await expect(row.getByTestId('identity-not-loaded')).toHaveCount(0)
  const sales = (await row.getAttribute('data-identity'))!

  // The signer joins its department on their team page (#147).
  await page.goto(`/team/${signer}`)
  const role = page.getByTestId('identity-access').getByLabel(`Role in ${label}`)
  await role.click()
  await page.getByRole('option', { name: 'Member', exact: true }).click()
  await expect(role).toContainText('Member')

  // A token for the signer signs as the new identity.
  const issued = await page.request.post('/api/bunker/connections/tokens', {
    headers: sameOrigin,
    data: { for_pubkey: signer, label: 'e2e sales app', kinds: [1], identity: sales }
  })
  expect(issued.status()).toBe(200)
  const url = new URL((await issued.json() as { uri: string }).uri)
  expect(url.hostname).toBe(sales)
  const app = BunkerSigner.fromBunker(generateSecretKey(), { pubkey: url.hostname, relays: [relay], secret: url.searchParams.get('secret') })
  await within(app.connect())
  const signed = await within(app.signEvent({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [], content: 'from e2e sales' }))
  expect(signed.pubkey).toBe(sales)

  // Retired after a confirmation naming it: it leaves Config, and the app is no longer answered.
  await page.goto('/config')
  await section.locator(`[data-identity="${sales}"]`).getByTestId('identity-retire').click()
  const confirm = section.getByTestId('identity-retire-confirm')
  await expect(confirm).toContainText(`Retire ${label}?`)
  await expect(confirm).toContainText('1 connected app')
  await confirm.getByRole('button', { name: `Retire ${label}` }).click()
  await expect(section.getByTestId('identity-retired')).toContainText(`Retired ${label}`)
  await expect(section.locator(`[data-identity="${sales}"]`)).toHaveCount(0)
  await expect(within(app.signEvent({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [], content: 'after retiring' }), 5_000)).rejects.toThrow()
  await app.close()

  // Its signature stays in the log, still naming it.
  const logs = await (await page.request.get('/api/bunker/logs')).json() as { event_id: string, identity_pubkey?: string }[]
  expect(logs.find(l => l.event_id === signed.id)?.identity_pubkey).toBe(sales)
})

test('an administrator edits the second identity\'s profile and follows, signed as it (#150)', async ({ page }) => {
  const brand = hexOf(process.env.E2E_BRAND_NPUB!)
  const followed = getPublicKey(generateSecretKey())
  const pool = new SimplePool()

  // From Config, to the identity's own profile page, which names it.
  await page.goto('/config')
  await page.getByTestId('identities-section').locator(`[data-identity="${brand}"]`).getByTestId('identity-edit-profile').click()
  await expect(page).toHaveURL(`/identities/${brand}/profile`)
  await expect(page.getByTestId('editing-identity')).toContainText('Editing e2e brand')
  await page.getByLabel('Name', { exact: true }).fill('e2e brand name')
  const confirmNew = page.getByLabel('I have no profile on another relay: create a new one.')
  if (await confirmNew.isVisible()) await confirmNew.check()
  await page.getByTestId('profile-save').click()
  await expect(page.getByTestId('profile-saved')).toContainText('Accepted by')

  const profiles = await pool.querySync([relay], { kinds: [0], authors: [brand] })
  const profile = profiles.sort((a, b) => b.created_at - a.created_at)[0]!
  expect(profile.pubkey).toBe(brand)
  expect(JSON.parse(profile.content).name).toBe('e2e brand name')

  // Its follows, the same way.
  await page.goto(`/identities/${brand}/follows`)
  await expect(page.getByTestId('editing-identity')).toContainText('Editing e2e brand')
  await page.getByLabel('Follow someone').fill(npubEncode(followed))
  await page.getByRole('button', { name: 'Find', exact: true }).click()
  await page.getByTestId('add-follow-preview').getByRole('button', { name: 'Add' }).click()
  const newList = page.getByLabel('I have no follow list on another relay: start a new one.')
  if (await newList.isVisible()) await newList.check()
  await page.getByTestId('follows-save').click()
  await expect(page.getByTestId('follows-saved')).toBeVisible()
  const lists = await pool.querySync([relay], { kinds: [3], authors: [brand] })
  expect(lists.some(list => list.tags.some(tag => tag[0] === 'p' && tag[1] === followed))).toBe(true)
  pool.close([relay])

  // Both are in the log under the administrator, from the console.
  const admin = getPublicKey(decode(process.env.E2E_ADMIN_NSEC!).data as Uint8Array)
  const logs = await (await page.request.get('/api/bunker/logs')).json() as { event_id: string, origin?: string, identity_pubkey?: string, member_pubkey?: string }[]
  for (const id of [profile.id, lists.find(list => list.tags.some(tag => tag[1] === followed))!.id]) {
    const edit = logs.find(l => l.event_id === id)
    expect(edit?.origin).toBe('console')
    expect(edit?.identity_pubkey).toBe(brand)
    expect(edit?.member_pubkey).toBe(admin)
  }
})
