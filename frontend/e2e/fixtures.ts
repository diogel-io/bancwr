import { test as base, expect } from '@playwright/test'

export { expect }

/**
 * `test`, failing any test in which the page throws or a /api/bunker/* request gets a 5xx. Those
 * are the failures a page can otherwise survive while still rendering its headings, such as the
 * proxy looping on itself (#20). A 4xx is left to the test: the invalid-pubkey case expects one.
 */
export const test = base.extend<{ bunkerNpub: string, guard: undefined }>({
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
