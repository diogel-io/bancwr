import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import DefaultLayout from '~/layouts/default.vue'

describe('default layout', () => {
  it('links to this repository, not the pre-transfer path', async () => {
    const component = await mountSuspended(DefaultLayout)
    const hrefs = component.findAll('a').map(a => a.attributes('href'))

    expect(hrefs).toContain('https://github.com/diogel-io/bancwr')
    // The old threenine path still resolves through GitHub's transfer redirect, so a wrong value
    // here would not look broken. Assert it is absent rather than relying on someone noticing.
    expect(hrefs.some(href => href?.includes('threenine'))).toBe(false)
  })
})
