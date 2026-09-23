import { describe, it, expect } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import Index from '../../app/pages/index.vue'

const mockStatus = { status: 'healthy', pubkey: 'npub1test' }
const mockLogs = [
  { id: '1', timestamp: '2025-01-01T10:00:00Z', kind: 1, member: 'Alice', status: 'success' }
]

describe('Index page', () => {
  it('renders the bunker status', async () => {
    registerEndpoint('/api/bunker/status', () => mockStatus)
    registerEndpoint('/api/bunker/logs', () => mockLogs)

    const component = await mountSuspended(Index)

    expect(component.text()).toContain('Bunker Status')
    expect(component.text()).toContain('healthy')
    expect(component.text()).toContain('npub1test')
  })

  it('does not carry a quick-actions card', async () => {
    registerEndpoint('/api/bunker/status', () => mockStatus)
    registerEndpoint('/api/bunker/logs', () => mockLogs)

    const component = await mountSuspended(Index)

    // Config and Team are permanent sidebar destinations, so the card was a second copy of
    // navigation that already exists. Asserted rather than merely deleted, so that re-adding it
    // is a deliberate act.
    expect(component.text()).not.toContain('Quick Actions')
    expect(component.text()).not.toContain('Configure Bunker')
    expect(component.text()).not.toContain('Manage Team')
  })

  it('shows activity logs', async () => {
    registerEndpoint('/api/bunker/status', () => mockStatus)
    registerEndpoint('/api/bunker/logs', () => mockLogs)

    const component = await mountSuspended(Index)

    expect(component.text()).toContain('Recent Activity')
    expect(component.text()).toContain('Alice')
  })
})
