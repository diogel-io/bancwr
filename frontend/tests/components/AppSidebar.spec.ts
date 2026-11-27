import { describe, it, expect, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { clearNuxtData } from '#imports'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import { signInAs } from '../helpers/session'
import AppSidebar from '~/components/AppSidebar.vue'

// The identities the bunker lists to the signed-in member: a lead's departments (#147).
const listed = vi.hoisted(() => ({ identities: [] as unknown[] }))
registerEndpoint('/api/bunker/identities', () => listed.identities)

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

    for (const label of ['Dashboard', 'Config', 'Team', 'Departments', 'Logs', 'Profile', 'Follows', 'Relays', 'Connections', 'Approvals']) {
      expect(component.text()).toContain(label)
    }
    expect(hrefs(component)).toEqual(['/', '/config', '/team', '/departments', '/logs', '/profile', '/follows', '/relays', '/connections', '/approvals'])
  })

  it('offers signers the dashboard, profile, follows, relays, connections and approvals (#102), never Config or Team (#143)', async () => {
    signInAs('signer')
    const component = await mountSuspended(AppSidebar)

    expect(hrefs(component)).toEqual(['/', '/profile', '/follows', '/relays', '/connections', '/approvals'])
    expect(component.text()).not.toContain('Config')
    expect(component.text()).not.toContain('Team')
  })

  it('marks Config, not the member\'s own Profile, on an identity\'s profile (#150)', async () => {
    signInAs('administrator')
    const active = async (route: string) => {
      const component = await mountSuspended(AppSidebar, { route })
      return component.findAll('a').filter(a => a.attributes('aria-current') === 'page' || a.attributes('data-active') !== undefined).map(a => a.attributes('href'))
    }
    expect(await active(`/identities/${'b'.repeat(64)}/profile`)).toEqual(['/config'])
    expect(await active('/profile')).toEqual(['/profile'])
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

  it('offers a department lead Departments as well, and still never Config or Team (#147)', async () => {
    listed.identities = [{ pubkey: 'c'.repeat(64), npub: 'npub1c', label: 'Sales', is_primary: false, available: true, members: [], leads: ['a'.repeat(64)], my_role: 'lead' }]
    await clearNuxtData('bunker-identities')
    signInAs('signer')
    const component = await mountSuspended(AppSidebar)
    await vi.waitFor(() => expect(hrefs(component)).toEqual(['/', '/departments', '/profile', '/follows', '/relays', '/connections', '/approvals']))
    listed.identities = []
    await clearNuxtData('bunker-identities')
  })
})
