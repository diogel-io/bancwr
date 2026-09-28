import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import Team from '../../app/pages/team.vue'
import { clearNuxtData, nextTick } from '#imports'

describe('Team page', () => {
  beforeEach(async () => {
    await clearNuxtData()
  })

  it('renders team page header and add button', async () => {
    registerEndpoint('/api/bunker/team', {
      method: 'GET',
      handler: () => []
    })

    const component = await mountSuspended(Team)

    expect(component.text()).toContain('Team Management')
    expect(component.text()).toContain('Add Member')
    // Should also show empty table
    expect(component.text()).toContain('No data')
  })

  it('renders TeamMemberList component with data', async () => {
    registerEndpoint('/api/bunker/team', {
      method: 'GET',
      handler: () => [
        { id: '1', name: 'Alice', pubkey: 'npub1', role: 'admin' }
      ]
    })

    const component = await mountSuspended(Team)

    expect(component.text()).toContain('Alice')
  })

  it('refreshes TeamMemberList after adding a member', async () => {
    const teamMembers = [
      { id: '1', name: 'Alice', pubkey: 'npub1', role: 'admin' }
    ]

    registerEndpoint('/api/bunker/team', {
      method: 'GET',
      handler: () => teamMembers
    })

    registerEndpoint('/api/bunker/team', {
      method: 'POST',
      handler: () => {
        teamMembers.push({ id: '2', name: 'Bob', pubkey: 'npub2', role: 'signer' })
        return { success: true }
      }
    })

    const component = await mountSuspended(Team)
    expect(component.text()).toContain('Alice')
    expect(component.text()).not.toContain('Bob')

    // Find and fill inputs
    const inputs = component.findAll('input')
    expect(inputs.length).toBeGreaterThanOrEqual(2)
    await inputs[0]!.setValue('Bob')
    await inputs[1]!.setValue('npub2')

    // Click add button
    const addButton = component.findAll('button').find(b => b.text().includes('Add Member'))
    await addButton!.trigger('click')

    // Wait for async actions and refresh
    await nextTick()
    await new Promise(resolve => setTimeout(resolve, 50))
    await nextTick()

    // Assert the list is refreshed and contains Bob
    expect(component.text()).toContain('Bob')
  })
})
