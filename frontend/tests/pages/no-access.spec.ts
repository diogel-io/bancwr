import { describe, it, expect, vi } from 'vitest'
import { mockNuxtImport, mountSuspended } from '@nuxt/test-utils/runtime'
import { ref } from 'vue'
import NoAccess from '~/pages/no-access.vue'

const { signOut } = vi.hoisted(() => ({ signOut: vi.fn() }))
mockNuxtImport('useAuth', () => () => ({
  state: ref({ status: 'not-registered', npub: 'npub1presentedkey' }),
  signOut
}))

describe('No-access page', () => {
  it('shows the presented npub and says whom to ask', async () => {
    const component = await mountSuspended(NoAccess)
    expect(component.text()).toContain('not registered with this bunker')
    expect(component.find('[data-testid="presented-npub"]').text()).toBe('npub1presentedkey')
    expect(component.text()).toContain('vault administrator')
  })

  it('offers sign-out rather than a retry loop', async () => {
    const component = await mountSuspended(NoAccess)
    const buttons = component.findAll('button')
    expect(buttons.some(b => /try again|retry/i.test(b.text()) && !/another key/i.test(b.text()))).toBe(false)
    await buttons.find(b => b.text().includes('Sign out and try another key'))!.trigger('click')
    expect(signOut).toHaveBeenCalled()
  })
})
