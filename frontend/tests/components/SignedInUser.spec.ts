import { describe, it, expect, vi } from 'vitest'
import { mockNuxtImport, mountSuspended } from '@nuxt/test-utils/runtime'
import { defineComponent, h, ref } from 'vue'
import { UApp } from '#components'
import SignedInUser from '~/components/SignedInUser.vue'

const npub = 'npub1wsmmysc6a60nk8dswq706zcvu7auzyz0869z9dryze5c8shd5z8qnd57dv'
const { signOut } = vi.hoisted(() => ({ signOut: vi.fn() }))
mockNuxtImport('useAuth', () => () => ({
  state: ref({ status: 'signed-in', pubkey: 'a'.repeat(64), npub, role: 'administrator' }),
  signOut
}))

describe('SignedInUser', () => {
  it('shows the signed-in key and role, and signs out', async () => {
    const component = await mountSuspended(SignedInUser)
    expect(component.text()).toContain('npub1wsmmysc…d57dv'.slice(0, 13))
    expect(component.text()).toContain('Administrator')
    expect(component.find('[role="group"]').attributes('aria-label')).toBe(`Signed in as ${npub}, Administrator`)
    await component.find('button[aria-label="Sign out"]').trigger('click')
    expect(signOut).toHaveBeenCalled()
  })

  it('keeps the identity in the accessible name when collapsed', async () => {
    // Collapsed, it renders a tooltip, which needs the provider UApp installs in app.vue.
    const Collapsed = defineComponent({ render: () => h(UApp, () => h(SignedInUser, { collapsed: true })) })
    const component = await mountSuspended(Collapsed)
    expect(component.text()).not.toContain('Administrator')
    expect(component.find('button').attributes('aria-label')).toBe(`Sign out. Signed in as ${npub}, Administrator`)
  })
})
