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

export type Role = 'administrator' | 'user' | 'signer'

// The keys global-setup.ts registered for each role.
const NSEC_VARIABLES: Record<Role, string> = {
  administrator: 'E2E_ADMIN_NSEC',
  user: 'E2E_USER_NSEC',
  signer: 'E2E_SIGNER_NSEC'
}

/**
 * `test`: every page starts signed in (#11), through the page's own request context, so the
 * browser holds the session cookie. As the seeded administrator unless a spec says
 * `test.use({ role: 'user' })` or `'signer'` (#26).
 */
export const test = anonymousTest.extend<{ role: Role, signedIn: undefined }>({
  role: ['administrator', { option: true }],

  signedIn: [async ({ page, role }, use) => {
    const nsec = process.env[NSEC_VARIABLES[role]]
    expect(nsec, `${NSEC_VARIABLES[role]} is set by global setup`).toBeTruthy()
    const login = await signInAs(nsec!, baseUrl(), contextPost(page.request))
    expect(login.status, `${role} sign-in: ${JSON.stringify(login.body)}`).toBe(200)
    await use(undefined)
  }, { auto: true }]
})
