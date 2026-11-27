// Departments (#147) through the real stack: an administrator makes the e2e signer the lead of the
// second identity's department on the Departments page. Signed in as that lead, in a browser
// context of their own, they see only that department, issue themselves a connection that signs as
// it with notes needing approval, and approve its held note on the Approvals page, while an
// administrator's held note on the primary is never listed to them and is refused if they try. The
// lead is removed from the department at the end, so later specs find the signer as before.
import { BunkerSigner } from 'nostr-tools/nip46'
import { decode } from 'nostr-tools/nip19'
import { generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { test, expect, sameOrigin, contextPost } from './fixtures'
import { signInAs } from './sign-in'
import { baseUrl } from './stack'

const relay = `ws://127.0.0.1:${process.env.E2E_RELAY_PORT || 7777}`

function within<T>(work: Promise<T>, ms = 15_000): Promise<T> {
  return Promise.race([work, new Promise<T>((_, reject) => setTimeout(() => reject(new Error('timed out')), ms))])
}

const note = (content: string) => ({ kind: 1, created_at: Math.floor(Date.now() / 1000), tags: [['t', 'e2e']], content })

/** A connected NIP-46 app for a token's bunker:// string, which waits through approval. */
async function appFor(uri: string): Promise<BunkerSigner> {
  const url = new URL(uri)
  const app = BunkerSigner.fromBunker(generateSecretKey(), { pubkey: url.hostname, relays: [relay], secret: url.searchParams.get('secret') }, { onauth: () => {} })
  await within(app.connect())
  return app
}

test.skip(!process.env.E2E_BRAND_NPUB, 'needs the stack started here, which adds the second identity')

test('an administrator appoints a lead, who manages their department and can\'t see another\'s approvals (#147)', async ({ page, browser }) => {
  const brand = decode(process.env.E2E_BRAND_NPUB!).data as string
  const signer = getPublicKey(decode(process.env.E2E_SIGNER_NSEC!).data as Uint8Array)

  // Appointed on the Departments page.
  await page.goto('/departments')
  const card = page.locator(`[data-department="${brand}"]`)
  await card.getByLabel('Add a team member to e2e brand').click()
  await page.getByRole('option', { name: 'e2e signer', exact: true }).click()
  await card.getByLabel('Role in e2e brand').click()
  await page.getByRole('option', { name: 'Lead', exact: true }).click()
  await card.getByTestId('department-add').click()
  await expect(card.locator(`[data-member="${signer}"]`).getByTestId('department-lead')).toBeVisible()

  const lead = await browser.newContext({ baseURL: baseUrl() })
  const apps: BunkerSigner[] = []
  try {
    // An administrator's own note on the primary, held for approval: not the lead's to see.
    const adminToken = await page.request.post('/api/bunker/connections/tokens', {
      headers: sameOrigin,
      data: { label: 'e2e primary held', kinds: [1], approval_kinds: [1] }
    })
    expect(adminToken.status()).toBe(200)
    const adminApp = await appFor((await adminToken.json() as { uri: string }).uri)
    apps.push(adminApp)
    const adminSigning = adminApp.signEvent(note('e2e: the primary\'s')).catch(() => undefined)
    await expect.poll(async () => (await (await page.request.get('/api/bunker/approvals')).json() as unknown[]).length, { timeout: 15_000 }).toBeGreaterThan(0)
    const held = (await (await page.request.get('/api/bunker/approvals')).json() as { id: string, event_id: string, content: string }[]).find(r => r.content === 'e2e: the primary\'s')!

    // Signed in as the lead.
    const leadPage = await lead.newPage()
    const login = await signInAs(process.env.E2E_SIGNER_NSEC!, baseUrl(), contextPost(leadPage.request))
    expect(login.status).toBe(200)
    await leadPage.goto('/departments')
    await expect(leadPage.getByTestId('department')).toHaveCount(1)
    await expect(leadPage.locator(`[data-department="${brand}"]`)).toBeVisible()

    // A connection that signs as their department, with notes needing approval.
    const issued = await leadPage.request.post('/api/bunker/connections/tokens', {
      headers: sameOrigin,
      data: { label: 'e2e lead app', kinds: [1], approval_kinds: [1], identity: brand }
    })
    expect(issued.status()).toBe(200)
    const leadApp = await appFor((await issued.json() as { uri: string }).uri)
    apps.push(leadApp)
    const signing = leadApp.signEvent(note('e2e: the department\'s'))

    // Only their department's request is listed, and they approve it.
    await leadPage.goto('/approvals')
    const cards = leadPage.locator('[data-approval]')
    await expect(cards.filter({ hasText: 'e2e: the department\'s' })).toBeVisible({ timeout: 15_000 })
    await expect(cards.filter({ hasText: 'e2e: the primary\'s' })).toHaveCount(0)
    await cards.filter({ hasText: 'e2e: the department\'s' }).getByTestId('approval-approve').click()
    const signed = await within(signing)
    expect(signed.pubkey).toBe(brand)

    // The administrator's is refused to them, as if it did not exist.
    const refused = await leadPage.request.post(`/api/bunker/approvals/${held.id}/approve`, { headers: sameOrigin, data: { event_id: held.event_id } })
    expect(refused.status()).toBe(404)
    const rejected = await page.request.post(`/api/bunker/approvals/${held.id}/reject`, { headers: sameOrigin, data: {} })
    expect(rejected.status()).toBe(200)
    await within(adminSigning)
  } finally {
    for (const app of apps) {
      await within(app.logout()).catch(() => undefined)
      await app.close()
    }
    await lead.close()
    // Out of the department again, so later specs find the signer with the primary only.
    await page.request.delete(`/api/bunker/identities/${brand}/members/${signer}`, { headers: sameOrigin })
  }
})
