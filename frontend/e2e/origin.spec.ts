import type { Page } from '@playwright/test'
import { test, expect, sameOrigin } from './fixtures'
import { randomNpub } from './keys'
import { baseUrl } from './stack'

// A signed-in administrator's cookie goes with every request from the same site, so the console
// accepts a request that changes something only from its own origin, and a body for the bunker
// only as JSON. page.request carries the session cookie, as the browser would, and sends no
// Origin unless a test gives one.

const otherOrigin = sameOrigin.Origin.replace('localhost', 'other.localhost')

async function teamNames(page: Page) {
  const team = await (await page.request.get('/api/bunker/team')).json() as { name: string }[]
  return team.map(member => member.name)
}

test('a request from another origin on the same site changes nothing', async ({ page }) => {
  const name = `Origin ${Date.now()}`
  const response = await page.request.post('/api/bunker/team', {
    headers: { Origin: otherOrigin },
    data: { name, pubkey: randomNpub(), role: 'administrator' }
  })

  expect(response.status()).toBe(403)
  expect(await response.json()).toMatchObject({ data: { error: 'forbidden', reason: 'bad_origin' } })
  expect(await teamNames(page)).not.toContain(name)
})

test('a request with the session cookie and neither Origin nor Sec-Fetch-Site changes nothing', async ({ page }) => {
  const name = `No origin ${Date.now()}`
  const response = await page.request.post('/api/bunker/team', {
    data: { name, pubkey: randomNpub(), role: 'administrator' }
  })
  expect(response.status()).toBe(403)
  expect(await response.json()).toMatchObject({ data: { error: 'forbidden', reason: 'bad_origin' } })

  // An empty body too, as an approval or a sign-out is.
  expect((await page.request.post('/api/auth/logout')).status()).toBe(403)
  expect((await page.request.get('/api/auth/session')).status()).toBe(200)
  expect(await teamNames(page)).not.toContain(name)
})

test('a request with neither header and no session cookie is let through, as a script sends', async () => {
  const response = await fetch(`${baseUrl()}/api/auth/challenge`, { method: 'POST' })
  expect(response.status).toBe(200)
})

test('a body that is not JSON, as a form sends, changes nothing', async ({ page }) => {
  const name = `Form ${Date.now()}`
  for (const path of ['/api/bunker/team', '/api/%62unker/team']) {
    const response = await page.request.post(path, {
      headers: { ...sameOrigin, 'Content-Type': 'text/plain' },
      data: JSON.stringify({ name, pubkey: randomNpub(), role: 'administrator' })
    })

    expect(response.status(), path).toBe(415)
  }
  expect(await teamNames(page)).not.toContain(name)
})

test('the console\'s own origin is accepted', async ({ page }) => {
  const name = `Same origin ${Date.now()}`
  const response = await page.request.post('/api/bunker/team', {
    headers: sameOrigin,
    data: { name, pubkey: randomNpub(), role: 'signer' }
  })

  expect(response.ok()).toBe(true)
  expect(await teamNames(page)).toContain(name)
})
