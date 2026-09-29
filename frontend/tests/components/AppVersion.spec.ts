import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { defineComponent, h } from 'vue'
import { clearNuxtData } from '#imports'
import { UApp } from '#components'
import type { BunkerStatus } from '#shared/types/bunker'
import AppVersion from '~/components/AppVersion.vue'

const status = (version: string): BunkerStatus => ({ status: 'healthy', pubkey: 'npub1test', version })

// Collapsed, it renders a tooltip, which needs the provider UApp installs in app.vue.
const Collapsed = defineComponent({
  render: () => h(UApp, () => h(AppVersion, { collapsed: true }))
})

describe('AppVersion', () => {
  // useFetch caches on its key, so without this each test would see the first test's payload.
  beforeEach(async () => {
    await clearNuxtData()
  })

  it('shows the bunker and frontend versions', async () => {
    registerEndpoint('/api/bunker/status', () => status('0.1.0-49'))
    registerEndpoint('/api/version', () => ({ version: '0.1.0-49' }))

    const component = await mountSuspended(AppVersion)

    expect(component.text()).toContain('Bunker 0.1.0-49')
    expect(component.text()).toContain('Frontend 0.1.0-49')
    expect(component.text()).not.toContain('differ')
  })

  it('warns when the two images are different versions', async () => {
    registerEndpoint('/api/bunker/status', () => status('0.1.0'))
    registerEndpoint('/api/version', () => ({ version: '0.1.1-3' }))

    const component = await mountSuspended(AppVersion)

    expect(component.text()).toContain('Bunker 0.1.0')
    expect(component.text()).toContain('Frontend 0.1.1-3')
    expect(component.text()).toContain('Bunker and frontend versions differ')
  })

  it('reports the bunker version as unknown when the bunker is unreachable', async () => {
    registerEndpoint('/api/bunker/status', () => {
      throw new Error('unreachable')
    })
    registerEndpoint('/api/version', () => ({ version: '0.1.0' }))

    const component = await mountSuspended(AppVersion)

    expect(component.text()).toContain('Bunker unknown')
    expect(component.text()).toContain('Frontend 0.1.0')
    // Unknown is not a mismatch.
    expect(component.text()).not.toContain('differ')
  })

  it('keeps the versions in the accessible name when the sidebar is collapsed', async () => {
    registerEndpoint('/api/bunker/status', () => status('0.1.0'))
    registerEndpoint('/api/version', () => ({ version: '0.1.1-3' }))

    const component = await mountSuspended(Collapsed)
    const group = component.find('[role="group"]')

    expect(component.text()).not.toContain('Bunker 0.1.0')
    expect(group.attributes('aria-label')).toBe(
      'Bunker 0.1.0, Frontend 0.1.1-3, Bunker and frontend versions differ'
    )
  })
})
