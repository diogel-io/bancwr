import { test as base, expect, type APIRequestContext } from '@playwright/test'
import { signInAs, type PostLike } from './sign-in'
import { baseUrl } from './stack'

export { expect }

/**
 * `anonymousTest`, not signed in, failing any test in which the page throws or a /api/bunker/* request gets a 5xx. Those
 * are the failures a page can otherwise survive while still rendering its headings, such as the
 * proxy looping on itself (#20). A 4xx is left to the test: the invalid-pubkey case expects one.
 */
export const anonymousTest = base.extend<{ bunkerNpub: string, guard: undefined }>({
  // The npub global-setup.ts derived from the bunker key it generated.
  // eslint-disable-next-line no-empty-pattern -- Playwright requires the destructuring pattern
  bunkerNpub: async ({}, use) => {
    await use(process.env.E2E_BUNKER_NPUB ?? '')
  },

  guard: [async ({ page }, use) => {
    const problems: string[] = []
    page.on('pageerror', error => problems.push(`page error: ${error.message}`))
    page.on('response', (response) => {
      if (response.url().includes('/api/bunker/') && response.status() >= 500) {
        problems.push(`${response.status()} from ${response.request().method()} ${response.url()}`)
      }
    })

    await use(undefined)

    expect(problems, 'page errors or bunker 5xx responses').toEqual([])
  }, { auto: true }]
})

/** POSTs through a Playwright request context, which shares its cookies with the page. */
export function contextPost(request: APIRequestContext): PostLike {
  return async (url, init) => {
    const response = await request.post(url, { headers: init.headers, data: init.body })
    return { status: response.status(), json: () => response.json() }
  }
}

/**
 * `test`: every page starts signed in as the seeded administrator (#11), through the page's own
 * request context, so the browser holds the session cookie.
 */
export const test = anonymousTest.extend<{ signedIn: undefined }>({
  signedIn: [async ({ page }, use) => {
    const login = await signInAs(process.env.E2E_ADMIN_NSEC!, baseUrl(), contextPost(page.request))
    expect(login.status, `administrator sign-in: ${JSON.stringify(login.body)}`).toBe(200)
    await use(undefined)
  }, { auto: true }]
})
