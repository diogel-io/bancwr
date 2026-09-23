import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import AppHeader from '~/components/AppHeader.vue'

describe('AppHeader', () => {
  it('renders navigation links', async () => {
    const component = await mountSuspended(AppHeader)

    // 'Bancwr', not 'Bancwr Diogel'. Bancwr is the product within the Diogel suite
    // (00-company/naming.md), so the suite name is carried by the mark beside it rather than
    // repeated in the wordmark.
    expect(component.text()).toContain('Bancwr')
    expect(component.text()).toContain('Dashboard')
    expect(component.text()).toContain('Config')
    expect(component.text()).toContain('Team')
    expect(component.text()).toContain('Logs')
  })

  it('shows the Diogel mark', async () => {
    const component = await mountSuspended(AppHeader)

    expect(component.findComponent({ name: 'DiogelLogo' }).exists()).toBe(true)
  })

  it('contains links to correct pages', async () => {
    const component = await mountSuspended(AppHeader)
    const links = component.findAllComponents({ name: 'NuxtLink' })

    const hrefs = links.map(link => link.props('to'))
    expect(hrefs).toContain('/')
    expect(hrefs).toContain('/config')
    expect(hrefs).toContain('/team')
    expect(hrefs).toContain('/logs')
  })
})
