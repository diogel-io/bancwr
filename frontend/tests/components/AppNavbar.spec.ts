import { describe, it, expect } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { defineComponent, h } from 'vue'
import { UApp, UDashboardGroup } from '#components'
import AppNavbar from '~/components/AppNavbar.vue'

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))

// The sidebar control needs the dashboard group, and the indicator's popover needs UApp.
const InDashboard = defineComponent({
  render: () => h(UApp, () => h(UDashboardGroup, () => h(AppNavbar, { title: 'Team Management' })))
})

describe('AppNavbar', () => {
  it('shows the title, with the health indicator top left beside the sidebar control', async () => {
    const component = await mountSuspended(InDashboard)

    expect(component.text()).toContain('Team Management')
    const indicator = component.find('[data-testid="bunker-health"]')
    expect(indicator.exists()).toBe(true)

    // In the leading slot, before the title.
    const html = component.html()
    expect(html.indexOf('data-testid="bunker-health"')).toBeLessThan(html.indexOf('Team Management'))
  })
})
