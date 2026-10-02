import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import type { Role } from '#shared/types/bunker'
import { signInAs } from '../helpers/session'
import AppSidebar from '~/components/AppSidebar.vue'

// Collapsed items render tooltips, which need the provider UApp installs in app.vue.
const InApp = (props: { collapsed?: boolean }) => defineComponent({
  render: () => h(UApp, () => h(AppSidebar, props))
})


const hrefs = (component: { findAll: (selector: 'a') => { attributes: (name: string) => string | undefined }[] }) =>
  component.findAll('a').map(a => a.attributes('href'))

describe('AppSidebar', () => {
  it('offers an administrator every Bancwr destination, each linked to its route', async () => {
    signInAs('administrator')
    const component = await mountSuspended(AppSidebar)

    for (const label of ['Dashboard', 'Config', 'Team', 'Logs', 'Profile']) {
      expect(component.text()).toContain(label)
    }
    expect(hrefs(component)).toEqual(['/', '/config', '/team', '/logs', '/profile'])
  })

  it('offers users and signers the dashboard and their own profile only (#26, #30)', async () => {
    for (const role of ['user', 'signer'] as Role[]) {
      signInAs(role)
      const component = await mountSuspended(AppSidebar)

      expect(hrefs(component), role).toEqual(['/', '/profile'])
      expect(component.text(), role).not.toContain('Config')
    }
  })

  it('offers nothing when no one is signed in', async () => {
    signInAs(undefined)
    const component = await mountSuspended(AppSidebar)

    expect(hrefs(component)).toEqual([])
  })

  it('keeps every destination reachable when collapsed', async () => {
    // Collapsing moves the labels into tooltips rather than dropping them, so the guard that
    // matters is that no destination is lost on the way.
    signInAs('administrator')
    const collapsed = await mountSuspended(InApp({ collapsed: true }))

    expect(hrefs(collapsed)).toEqual(expect.arrayContaining(['/', '/config', '/team', '/logs']))
  })
})
