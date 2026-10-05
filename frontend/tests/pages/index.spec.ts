import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { createError } from 'h3'
import type { BunkerStatus, LogEntry, Nip46Connection } from '#shared/types/bunker'
import { signInAs } from '../helpers/session'
import Index from '../../app/pages/index.vue'
import { clearNuxtData, useState } from '#imports'

const mockStatus: BunkerStatus = { status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }
// The real LogEntry shape from backend/src/server.rs.
const mockLogs: LogEntry[] = [
  { id: '1', event_id: 'e1', pubkey: 'npub1alice', event_kind: 1, timestamp: '2025-01-01T10:00:00Z' }
]
const mockMetrics = { http_requests: 42, nip46_connections: 3, total_signatures: 1234 }

// What each endpoint was asked for, so a role's view can be shown not to request the rest.
let requested: string[] = []

// The signer's own (#77): two active apps and an ended one, and their signatures.
const ago = (ms: number) => new Date(Date.now() - ms).toISOString()
const mockConnections: Partial<Nip46Connection>[] = [
  { id: 'c1', client_pubkey: 'c'.repeat(64), client_name: 'Damus', connected_at: ago(7_200_000), revoked_at: null },
  { id: 'c2', client_pubkey: 'd'.repeat(64), client_name: 'Amethyst', connected_at: ago(60_000), revoked_at: null },
  { id: 'c3', client_pubkey: 'e'.repeat(64), client_name: 'Old', connected_at: ago(30_000), revoked_at: ago(10_000) }
]
const mockMyLogs: LogEntry[] = [
  { id: '2', event_id: 'e2', pubkey: 'c'.repeat(64), event_kind: 1, timestamp: '2025-01-02T10:00:00Z', member_pubkey: 'a'.repeat(64), member_name: 'Gary', connection_id: 'c1' }
]

function serve(admin: { logs?: () => unknown, connections?: () => unknown } = {}, status: BunkerStatus = mockStatus) {
  requested = []
  registerEndpoint('/api/bunker/status', () => {
    requested.push('status')
    return status
  })
  registerEndpoint('/api/bunker/logs', () => {
    requested.push('logs')
    return admin.logs ? admin.logs() : mockLogs
  })
  registerEndpoint('/api/bunker/metrics', () => {
    requested.push('metrics')
    return mockMetrics
  })
  registerEndpoint('/api/bunker/connections', () => {
    requested.push('connections')
    return admin.connections ? admin.connections() : mockConnections
  })
  registerEndpoint('/api/bunker/logs/mine', () => {
    requested.push('logs/mine')
    return mockMyLogs
  })
  registerEndpoint('/api/bunker/team', () => {
    requested.push('team')
    return []
  })
}


describe('Index page', () => {
  beforeEach(async () => {
    await clearNuxtData()
    // The health state is shared with the navbar indicator (#28), and outlives a mount.
    useState('bunker-health').value = { state: 'unknown', checks: [] }
  })

  describe('for an administrator', () => {
    beforeEach(() => signInAs('administrator'))

    it('renders the metrics, the bunker status and recent activity', async () => {
      serve()
      const component = await mountSuspended(Index)

      expect(component.text()).toContain('Total Signatures')
      expect(component.text()).toContain('1234')
      expect(component.text()).toContain('Bunker Status')
      expect(component.text()).toContain('healthy')
      expect(component.text()).toContain('npub1test')
      expect(component.text()).toContain('Recent Activity')
      expect(component.text()).toContain('npub1alice')
      // Unchanged by #77: no signer cards, and none of their requests.
      expect(component.find('[data-testid="my-connections"]').exists()).toBe(false)
      expect(requested).not.toContain('logs/mine')
      expect(requested).not.toContain('connections')
    })

    it('does not carry a quick-actions card', async () => {
      serve()
      const component = await mountSuspended(Index)

      // Config and Team are permanent sidebar destinations, so the card was a second copy of
      // navigation that already exists. Asserted rather than merely deleted, so that re-adding it
      // is a deliberate act.
      expect(component.text()).not.toContain('Quick Actions')
      expect(component.text()).not.toContain('Configure Bunker')
      expect(component.text()).not.toContain('Manage Team')
    })

    it('explains a bunker refusal of the activity, rather than showing an empty table', async () => {
      serve({ logs: () => { throw createError({ status: 403, data: { error: 'forbidden' } }) } })
      const component = await mountSuspended(Index)

      expect(component.find('[data-testid="forbidden-notice"]').exists()).toBe(true)
      expect(component.text()).toContain('Your role no longer allows this page')
    })
  })

  describe('for a signer (#77)', () => {
    beforeEach(() => signInAs('signer'))

    it('shows the health, their connected apps and their own recent signatures, never the administrator\'s data', async () => {
      serve()
      const component = await mountSuspended(Index)

      expect(component.text()).toContain('Bunker Status')
      expect(component.find('[data-testid="role-summary"]').text()).toContain('Signer')
      // Two active apps; the ended one is not counted. The latest is the most recently connected.
      expect(component.find('[data-testid="my-connections-count"]').text()).toBe('2')
      expect(component.find('[data-testid="my-latest-connection"]').text()).toContain('Amethyst')
      expect(component.find('[data-testid="my-signatures"]').text()).toContain('Gary')
      expect(component.find('[data-testid="my-signatures"]').text()).not.toContain('npub1alice')
      expect(component.text()).not.toContain('Total Signatures')
      expect(component.text()).not.toContain('Recent Activity')
      expect(requested).toEqual(expect.arrayContaining(['status', 'connections', 'logs/mine']))
      expect(requested).not.toContain('logs')
      expect(requested).not.toContain('metrics')
      expect(requested).not.toContain('team')
    })

    it('says when no app is connected for them', async () => {
      serve({ connections: () => [] })
      const component = await mountSuspended(Index)

      expect(component.find('[data-testid="my-connections-count"]').text()).toBe('0')
      expect(component.find('[data-testid="my-connections"]').text()).toContain('Ask an administrator')
    })
  })

  describe('for a viewer (#77)', () => {
    beforeEach(() => signInAs('viewer'))

    it('shows the health and a way to the team, and requests nothing else', async () => {
      serve()
      const component = await mountSuspended(Index)

      expect(component.text()).toContain('Bunker Status')
      expect(component.find('[data-testid="role-summary"]').text()).toContain('Viewer')
      expect(component.find('[data-testid="viewer-team"] a').attributes('href')).toBe('/team')
      expect(component.find('[data-testid="my-connections"]').exists()).toBe(false)
      expect(component.find('[data-testid="my-signatures"]').exists()).toBe(false)
      expect(component.text()).not.toContain('Total Signatures')
      expect(requested).toEqual(['status'])
    })
  })

  // Yellow is reachable now that the bunker reports degraded (#27).
  describe('status dot', () => {
    beforeEach(() => signInAs('signer'))

    for (const [state, colour] of [['healthy', 'bg-success'], ['degraded', 'bg-warning'], ['unhealthy', 'bg-error']] as const) {
      it(`is ${colour} when the bunker is ${state}`, async () => {
        serve({}, { ...mockStatus, status: state })
        const component = await mountSuspended(Index)

        expect(component.find('[data-testid="status-dot"]').classes()).toContain(colour)
        expect(component.text()).toContain(state)
      })
    }
  })
})
