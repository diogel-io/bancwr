import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport, mountSuspended } from '@nuxt/test-utils/runtime'
import { ref } from 'vue'
import NoAccess from '~/pages/no-access.vue'

const { signOut, auth } = vi.hoisted(() => ({ signOut: vi.fn(), auth: { noAdministrator: false } }))
mockNuxtImport('useAuth', () => () => ({
  state: ref({ status: 'not-registered', npub: 'npub1presentedkey', ...(auth.noAdministrator ? { noAdministrator: true } : {}) }),
  signOut
}))

describe('No-access page', () => {
  beforeEach(() => {
    auth.noAdministrator = false
  })

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

  it('says how the first administrator is set when the bunker has none (#74)', async () => {
    auth.noAdministrator = true
    const component = await mountSuspended(NoAccess)
    const guidance = component.find('[data-testid="no-administrator"]').text()
    expect(guidance).toContain('no administrator yet')
    expect(guidance).toContain('BANCWR_ADMIN_PUBKEY')
    expect(guidance).toContain('BUNKER_NSEC')
    expect(component.text()).not.toContain('Contact the vault')
    expect(component.find('[data-testid="presented-npub"]').text()).toBe('npub1presentedkey')
  })

  it('keeps the usual text when there is an administrator', async () => {
    const component = await mountSuspended(NoAccess)
    expect(component.find('[data-testid="no-administrator"]').exists()).toBe(false)
  })
})
