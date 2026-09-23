import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import AppSidebar from '~/components/AppSidebar.vue'

// Collapsed items render tooltips, which need the provider UApp installs in app.vue.
const InApp = (props: { collapsed?: boolean }) => defineComponent({
  render: () => h(UApp, () => h(AppSidebar, props))
})

describe('AppSidebar', () => {
  it('offers the four Bancwr destinations', async () => {
    const component = await mountSuspended(AppSidebar)

    expect(component.text()).toContain('Dashboard')
    expect(component.text()).toContain('Config')
    expect(component.text()).toContain('Team')
    expect(component.text()).toContain('Logs')
  })

  it('links each destination to its route', async () => {
    const component = await mountSuspended(AppSidebar)
    const hrefs = component.findAll('a').map(a => a.attributes('href'))

    expect(hrefs).toContain('/')
    expect(hrefs).toContain('/config')
    expect(hrefs).toContain('/team')
    expect(hrefs).toContain('/logs')
  })

  it('keeps every destination reachable when collapsed', async () => {
    // Collapsing moves the labels into tooltips rather than dropping them, so the guard that
    // matters is that no destination is lost on the way.
    const collapsed = await mountSuspended(InApp({ collapsed: true }))
    const hrefs = collapsed.findAll('a').map(a => a.attributes('href'))

    expect(hrefs).toEqual(expect.arrayContaining(['/', '/config', '/team', '/logs']))
  })
})
