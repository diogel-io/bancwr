import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mountSuspended, registerEndpoint, mockNuxtImport } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { readBody } from 'h3'
import TeamMemberList from '~/components/TeamMemberList.vue'
import type { TeamMember } from '#shared/types/bunker'

const { addToast } = vi.hoisted(() => ({ addToast: vi.fn() }))

mockNuxtImport('useToast', () => () => ({ add: addToast }))

// The real TeamMember shape from backend/src/server.rs: ids are UUIDs.
const mockTeam: TeamMember[] = [
  { id: '5b0f3c2e-8a51-4d6e-9f3a-1c2b3d4e5f60', name: 'Alice', pubkey: 'a'.repeat(64), npub: 'npub1alice', role: 'administrator' },
  { id: 'c7d8e9f0-1a2b-4c3d-8e4f-5a6b7c8d9e0f', name: 'Bob', pubkey: 'b'.repeat(64), npub: 'npub1bob', role: 'signer' }
]

const bob = mockTeam[1]!

describe('TeamMemberList', () => {
  beforeEach(() => {
    addToast.mockClear()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('renders list from props', async () => {
    const component = await mountSuspended(TeamMemberList, {
      props: {
        data: mockTeam
      }
    })

    expect(component.text()).toContain('Alice')
    expect(component.text()).toContain('Bob')
    // The npub, not the stored hex, and the role's label, not its value (#24).
    expect(component.text()).toContain('npub1alice')
    expect(component.text()).not.toContain('a'.repeat(64))
    expect(component.text()).toContain('Admin')
    expect(component.text()).toContain('Signer')
  })

  it('shows the role names of #77', async () => {
    const viewer: TeamMember = { id: 'e1f2a3b4-5c6d-4e7f-8091-a2b3c4d5e6f7', name: 'Vera', pubkey: 'c'.repeat(64), npub: 'npub1vera', role: 'viewer' }
    const component = await mountSuspended(TeamMemberList, { props: { data: [...mockTeam, viewer] } })
    const roles = component.findAll('tbody tr').map(row => row.findAll('td')[2]!.text())
    expect(roles).toEqual(['Admin', 'Signer', 'Viewer'])
  })

  it('links each member to their read-only profile, by npub (#77)', async () => {
    const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })
    const link = component.get('a[aria-label="View Bob\'s profile"]')
    expect(link.attributes('href')).toBe('/team/npub1bob')
    expect(link.text()).toBe('Bob')
  })

  it('read-only: lists names, npubs and roles, with no remove control (#77)', async () => {
    const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam, readOnly: true } })

    expect(component.text()).toContain('Alice')
    expect(component.text()).toContain('npub1bob')
    expect(component.text()).toContain('Signer')
    expect(component.find('button[aria-label="Remove Bob"]').exists()).toBe(false)
    expect(component.text()).not.toContain('Actions')
    // Still a way to each member's profile.
    expect(component.get('a[aria-label="View Bob\'s profile"]').attributes('href')).toBe('/team/npub1bob')
  })

  it('shows a row from before #24 as stored', async () => {
    // An unmigratable key has no npub, and an unrecognised role has no label.
    const legacy: TeamMember = { id: 'd1e2f3a4-5b6c-4d7e-8f90-a1b2c3d4e5f6', name: 'Oscar', pubkey: 'npub1notakey', npub: null, role: 'owner' }
    const component = await mountSuspended(TeamMemberList, { props: { data: [legacy] } })

    expect(component.text()).toContain('npub1notakey')
    expect(component.text()).toContain('owner')
  })

  it('handles empty state', async () => {
    const component = await mountSuspended(TeamMemberList, {
      props: {
        data: []
      }
    })
    expect(component.text()).toContain('No data')
  })

  it('removes a member by their id, not the table row index', async () => {
    vi.stubGlobal('confirm', vi.fn(() => true))
    const deleted: string[] = []
    registerEndpoint(`/api/bunker/team/${bob.id}`, {
      method: 'DELETE',
      handler: (event) => {
        deleted.push(event.path)
        return { success: true, message: 'Team member removed' }
      }
    })

    const component = await mountSuspended(TeamMemberList, {
      props: {
        data: mockTeam
      }
    })

    await component.get('button[aria-label="Remove Bob"]').trigger('click')
    await flushPromises()

    // The test runner serves registered endpoints under an internal prefix, so match the suffix.
    expect(deleted).toHaveLength(1)
    expect(deleted[0]).toMatch(new RegExp(`/api/bunker/team/${bob.id}$`))
    expect(component.emitted('refresh')).toHaveLength(1)
    expect(addToast).toHaveBeenCalledWith(expect.objectContaining({ color: 'success' }))
  })

  it('sends nothing when the removal is not confirmed', async () => {
    vi.stubGlobal('confirm', vi.fn(() => false))
    const deleted: string[] = []
    registerEndpoint(`/api/bunker/team/${bob.id}`, {
      method: 'DELETE',
      handler: (event) => {
        deleted.push(event.path)
        return { success: true, message: 'Team member removed' }
      }
    })

    const component = await mountSuspended(TeamMemberList, {
      props: {
        data: mockTeam
      }
    })

    await component.get('button[aria-label="Remove Bob"]').trigger('click')
    await flushPromises()

    expect(deleted).toEqual([])
    expect(component.emitted('refresh')).toBeUndefined()
    expect(addToast).not.toHaveBeenCalled()
  })

  it('shows an error and does not refresh when the removal fails', async () => {
    vi.stubGlobal('confirm', vi.fn(() => true))
    registerEndpoint(`/api/bunker/team/${bob.id}`, {
      method: 'DELETE',
      handler: (event) => {
        event.node.res.statusCode = 404
        return 'Team member not found'
      }
    })

    const component = await mountSuspended(TeamMemberList, {
      props: {
        data: mockTeam
      }
    })

    await component.get('button[aria-label="Remove Bob"]').trigger('click')
    await flushPromises()

    expect(component.emitted('refresh')).toBeUndefined()
    expect(addToast).toHaveBeenCalledWith(expect.objectContaining({ color: 'error' }))
  })

  describe('changing a role (#99)', () => {
    /** Opens the member's role select and picks a role by its label. */
    async function choose(component: Awaited<ReturnType<typeof mountSuspended>>, name: string, label: string) {
      await component.get(`button[aria-label="Role of ${name}"]`).trigger('click')
      await flushPromises()
      // The newest list: one a previous test left open in the body is not this member's.
      const option = await vi.waitFor(() => {
        const list = [...document.body.querySelectorAll('[role="listbox"]')].pop()
        const found = [...(list?.querySelectorAll('[role="option"]') ?? [])].find(o => o.textContent?.trim() === label)
        expect(found).toBeDefined()
        return found as HTMLElement
      })
      // reka-ui selects an option on pointerup.
      option.dispatchEvent(new Event('pointerup', { bubbles: true }))
      await flushPromises()
    }

    function patchEndpoint(status = 200) {
      const sent: unknown[] = []
      registerEndpoint(`/api/bunker/team/${bob.id}`, {
        method: 'PATCH',
        handler: async (event) => {
          sent.push(await readBody(event))
          if (status !== 200) {
            event.node.res.statusCode = status
            return 'refused'
          }
          return { success: true, message: 'Role changed' }
        }
      })
      return sent
    }

    it('offers each member\'s role as a select of the three roles', async () => {
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })
      expect(component.get('button[aria-label="Role of Bob"]').text()).toContain('Signer')
      await component.get('button[aria-label="Role of Bob"]').trigger('click')
      await vi.waitFor(() => expect(document.body.querySelectorAll('[role="option"]').length).toBe(3))
      const options = [...document.body.querySelectorAll('[role="option"]')].map(o => o.textContent?.trim())
      expect(options).toEqual(['Admin', 'Signer', 'Viewer'])
    })

    it('read-only: no role select', async () => {
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam, readOnly: true } })
      expect(component.find('button[aria-label="Role of Bob"]').exists()).toBe(false)
      expect(component.find('button[role="combobox"]').exists()).toBe(false)
    })

    it('changes a role in one step, without asking, when the member can still sign', async () => {
      const ask = vi.fn(() => true)
      vi.stubGlobal('confirm', ask)
      const sent = patchEndpoint()
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })

      await choose(component, 'Bob', 'Admin')

      await vi.waitFor(() => expect(sent).toEqual([{ role: 'administrator' }]))
      expect(ask).not.toHaveBeenCalled()
      await vi.waitFor(() => expect(component.emitted('refresh')).toHaveLength(1))
      expect(addToast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Bob is now Admin', color: 'success' }))
    })

    it('asks before making a signer a viewer, saying their apps will be disconnected', async () => {
      const ask = vi.fn(() => true)
      vi.stubGlobal('confirm', ask)
      const sent = patchEndpoint()
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })

      await choose(component, 'Bob', 'Viewer')

      expect(ask).toHaveBeenCalledWith(expect.stringContaining('Make Bob a viewer?'))
      expect(ask).toHaveBeenCalledWith(expect.stringContaining('disconnected'))
      await vi.waitFor(() => expect(sent).toEqual([{ role: 'viewer' }]))
    })

    it('sends nothing, and shows the stored role again, when the downgrade is not confirmed', async () => {
      vi.stubGlobal('confirm', vi.fn(() => false))
      const sent = patchEndpoint()
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })

      await choose(component, 'Bob', 'Viewer')

      expect(sent).toEqual([])
      expect(component.emitted('refresh')).toBeUndefined()
      await vi.waitFor(() => expect(component.get('button[aria-label="Role of Bob"]').text()).toContain('Signer'))
    })

    it('says the last administrator cannot be demoted when the bunker refuses with 409', async () => {
      vi.stubGlobal('confirm', vi.fn(() => true))
      patchEndpoint(409)
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })

      await choose(component, 'Bob', 'Admin')

      await vi.waitFor(() => expect(addToast).toHaveBeenCalledWith(expect.objectContaining({ title: 'The last administrator cannot be demoted', color: 'error' })))
      expect(component.emitted('refresh')).toBeUndefined()
      await vi.waitFor(() => expect(component.get('button[aria-label="Role of Bob"]').text()).toContain('Signer'))
    })

    it('shows a general error for any other failure', async () => {
      vi.stubGlobal('confirm', vi.fn(() => true))
      patchEndpoint(500)
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })

      await choose(component, 'Bob', 'Admin')

      await vi.waitFor(() => expect(addToast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Failed to change role', color: 'error' })))
    })

    it('re-reads the session after changing the signed-in member\'s own role', async () => {
      vi.stubGlobal('confirm', vi.fn(() => true))
      patchEndpoint()
      registerEndpoint('/api/auth/session', () => ({ pubkey: bob.pubkey, npub: 'npub1bob', role: 'administrator' }))
      useAuth().state.value = { status: 'signed-in', pubkey: bob.pubkey, npub: 'npub1bob', role: 'signer' }
      const component = await mountSuspended(TeamMemberList, { props: { data: mockTeam } })

      await choose(component, 'Bob', 'Admin')

      await vi.waitFor(() => expect(useAuth().state.value).toMatchObject({ status: 'signed-in', role: 'administrator' }))
      useAuth().state.value = { status: 'signed-out' }
    })
  })
})
