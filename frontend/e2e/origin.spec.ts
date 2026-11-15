import { test, expect } from './fixtures'
import { randomNpub } from './keys'

// A signed-in administrator's cookie goes with every request from the same site, so the console
// accepts a request that changes something only from its own origin, and a body for the bunker
// only as JSON. page.request carries the session cookie, as the browser would.

const ORIGIN = 'http://localhost:3100'

test('a request from another origin on the same site changes nothing', async ({ page, baseURL }) => {
  const name = `Origin ${Date.now()}`
  const response = await page.request.post('/api/bunker/team', {
    headers: { 'Origin': new URL(baseURL ?? ORIGIN).origin.replace('localhost', 'other.localhost') },
    data: { name, pubkey: randomNpub(), role: 'administrator' }
  })

  expect(response.status()).toBe(403)
  expect(await response.json()).toMatchObject({ data: { error: 'forbidden', reason: 'bad_origin' } })
  const team = await (await page.request.get('/api/bunker/team')).json() as { name: string }[]
  expect(team.map(member => member.name)).not.toContain(name)
})

test('a body that is not JSON, as a form sends, changes nothing', async ({ page, baseURL }) => {
  const name = `Form ${Date.now()}`
  const response = await page.request.post('/api/bunker/team', {
    headers: { 'Origin': new URL(baseURL ?? ORIGIN).origin, 'Content-Type': 'text/plain' },
    data: JSON.stringify({ name, pubkey: randomNpub(), role: 'administrator' })
  })

  expect(response.status()).toBe(415)
  const team = await (await page.request.get('/api/bunker/team')).json() as { name: string }[]
  expect(team.map(member => member.name)).not.toContain(name)
})

test('the console\'s own origin is accepted', async ({ page, baseURL }) => {
  const name = `Same origin ${Date.now()}`
  const response = await page.request.post('/api/bunker/team', {
    headers: { 'Origin': new URL(baseURL ?? ORIGIN).origin },
    data: { name, pubkey: randomNpub(), role: 'signer' }
  })

  expect(response.ok()).toBe(true)
})
