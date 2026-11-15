import { describe, it, expect } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import DefaultLayout from '~/layouts/default.vue'

describe('default layout', () => {
  // The sidebar header shows the bunker's version (#124).
  registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0' }))

  it('links to this repository, not the pre-transfer path', async () => {
    const component = await mountSuspended(DefaultLayout)
    const hrefs = component.findAll('a').map(a => a.attributes('href'))

    expect(hrefs).toContain('https://github.com/diogel-io/bancwr')
    // The old threenine path still resolves through GitHub's transfer redirect, so a wrong value
    // here would not look broken. Assert it is absent rather than relying on someone noticing.
    expect(hrefs.some(href => href?.includes('threenine'))).toBe(false)
  })

  it('shows one version under the name in the sidebar header, and none in the footer', async () => {
    const component = await mountSuspended(DefaultLayout)

    expect(component.find('[data-testid="app-version"]').text()).toBe('V 0.1.0')
    expect(component.text()).not.toContain('Bunker 0.1.0')
    expect(component.text()).not.toContain('Frontend 0.1.0')
  })
})
