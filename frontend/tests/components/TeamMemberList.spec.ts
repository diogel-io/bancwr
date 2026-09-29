import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mountSuspended, registerEndpoint, mockNuxtImport } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import TeamMemberList from '~/components/TeamMemberList.vue'
import type { TeamMember } from '#shared/types/bunker'

const { addToast } = vi.hoisted(() => ({ addToast: vi.fn() }))

mockNuxtImport('useToast', () => () => ({ add: addToast }))

// The real TeamMember shape from backend/src/server.rs: ids are UUIDs.
const mockTeam: TeamMember[] = [
  { id: '5b0f3c2e-8a51-4d6e-9f3a-1c2b3d4e5f60', name: 'Alice', pubkey: 'npub1alice', role: 'admin' },
  { id: 'c7d8e9f0-1a2b-4c3d-8e4f-5a6b7c8d9e0f', name: 'Bob', pubkey: 'npub1bob', role: 'signer' }
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
    expect(component.text()).toContain('admin')
    expect(component.text()).toContain('signer')
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
})
