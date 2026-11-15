import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { defineComponent, h } from 'vue'
import { clearNuxtData } from '#imports'
import { UApp } from '#components'
import type { BunkerStatus } from '#shared/types/bunker'
import AppBrand from '~/components/AppBrand.vue'

const status = (version: string): BunkerStatus => ({ status: 'healthy', pubkey: 'npub1test', version, checks: [] })

// Collapsed, it renders a tooltip, which needs the provider UApp installs in app.vue.
const Collapsed = defineComponent({
  render: () => h(UApp, () => h(AppBrand, { collapsed: true }))
})

describe('AppBrand', () => {
  // useFetch caches on its key, so without this each test would see the first test's payload.
  beforeEach(async () => {
    await clearNuxtData()
  })

  it('shows the product name and the bunker version beneath it', async () => {
    registerEndpoint('/api/bunker/status', () => status('0.1.0-49'))

    const component = await mountSuspended(AppBrand)

    expect(component.text()).toContain('Bancwr')
    expect(component.find('[data-testid="app-version"]').text()).toBe('V 0.1.0-49')
    expect(component.find('a').attributes('href')).toBe('/')
  })

  it('shows the version as unknown when the bunker is unreachable', async () => {
    registerEndpoint('/api/bunker/status', () => {
      throw new Error('unreachable')
    })

    const component = await mountSuspended(AppBrand)

    expect(component.find('[data-testid="app-version"]').text()).toBe('V unknown')
  })

  it('shows one version, with no frontend version or mismatch warning (#124)', async () => {
    registerEndpoint('/api/bunker/status', () => status('0.1.0'))
    // Were it still read, a different frontend version would show up here.
    registerEndpoint('/api/version', () => ({ version: '0.1.1-3' }))

    const component = await mountSuspended(AppBrand)

    expect(component.text()).not.toContain('0.1.1-3')
    expect(component.text()).not.toContain('Frontend')
    expect(component.text()).not.toContain('differ')
  })

  it('keeps the name and version in the accessible name when the sidebar is collapsed', async () => {
    registerEndpoint('/api/bunker/status', () => status('0.1.0'))

    const component = await mountSuspended(Collapsed)
    const link = component.find('a')

    expect(component.text()).not.toContain('V 0.1.0')
    expect(link.attributes('aria-label')).toBe('Bancwr V 0.1.0')
  })
})
