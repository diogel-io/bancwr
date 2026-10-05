import { describe, it, expect, beforeEach, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import Team from '../../app/pages/team/index.vue'
import { clearNuxtData } from '#imports'
import { createError, readBody } from 'h3'
import { signInAs } from '../helpers/session'

describe('Team page', () => {
  beforeEach(async () => {
    await clearNuxtData()
    signInAs('administrator')
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
        { id: '1', name: 'Alice', pubkey: 'a'.repeat(64), npub: 'npub1alice', role: 'administrator' }
      ]
    })

    const component = await mountSuspended(Team)

    expect(component.text()).toContain('Alice')
  })

  it('refreshes TeamMemberList after adding a member', async () => {
    const teamMembers = [
      { id: '1', name: 'Alice', pubkey: 'a'.repeat(64), npub: 'npub1alice', role: 'administrator' }
    ]

    registerEndpoint('/api/bunker/team', {
      method: 'GET',
      handler: () => teamMembers
    })

    let posted: unknown
    registerEndpoint('/api/bunker/team', {
      method: 'POST',
      handler: async (event) => {
        posted = await readBody(event)
        teamMembers.push({ id: '2', name: 'Bob', pubkey: 'b'.repeat(64), npub: 'npub1bob', role: 'signer' })
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

    // Wait for the POST and the refresh to land, rather than for a fixed time: a 50 ms sleep here
    // failed whenever the whole suite ran under load.
    await vi.waitFor(() => expect(component.text()).toContain('Bob'))
    // The default role is sent with its settled value (#24, #77).
    expect(posted).toEqual({ name: 'Bob', pubkey: 'npub2', role: 'signer' })
  })

  it('offers the roles Admin, Signer and Viewer when adding (#77)', async () => {
    registerEndpoint('/api/bunker/team', { method: 'GET', handler: () => [] })
    const component = await mountSuspended(Team, { attachTo: document.body })

    await component.get('[data-testid="add-member"] button[role="combobox"]').trigger('click')
    await vi.waitFor(() => expect(document.body.querySelectorAll('[role="option"]').length).toBe(3))
    const options = [...document.body.querySelectorAll('[role="option"]')].map(o => o.textContent?.trim())
    expect(options).toEqual(['Admin', 'Signer', 'Viewer'])
    component.unmount()
  })

  it('shows a viewer the list read-only, with no add or remove controls (#77)', async () => {
    signInAs('viewer')
    registerEndpoint('/api/bunker/team', {
      method: 'GET',
      handler: () => [
        { id: '1', name: 'Alice', pubkey: 'a'.repeat(64), npub: 'npub1alice', role: 'administrator' },
        { id: '2', name: 'Bob', pubkey: 'b'.repeat(64), npub: 'npub1bob', role: 'signer' }
      ]
    })

    const component = await mountSuspended(Team)

    expect(component.text()).toContain('Alice')
    expect(component.text()).toContain('npub1bob')
    expect(component.text()).toContain('Signer')
    expect(component.text()).not.toContain('Team Management')
    expect(component.find('[data-testid="add-member"]').exists()).toBe(false)
    expect(component.text()).not.toContain('Add Member')
    expect(component.find('button[aria-label="Remove Bob"]').exists()).toBe(false)
    expect(component.find('a[aria-label="View Bob\'s profile"]').attributes('href')).toBe('/team/npub1bob')
  })

  it('explains a bunker 403 rather than breaking (#26)', async () => {
    registerEndpoint('/api/bunker/team', () => {
      throw createError({ status: 403, data: { error: 'forbidden' } })
    })

    const component = await mountSuspended(Team)

    expect(component.find('[data-testid="forbidden-notice"]').text()).toContain('Your role no longer allows this page')
  })
})
