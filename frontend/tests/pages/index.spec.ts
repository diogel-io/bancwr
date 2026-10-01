import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { createError } from 'h3'
import type { BunkerStatus, LogEntry, Role } from '#shared/types/bunker'
import { signInAs } from '../helpers/session'
import Index from '../../app/pages/index.vue'
import { clearNuxtData } from '#imports'

const mockStatus: BunkerStatus = { status: 'healthy', pubkey: 'npub1test', version: '0.1.0' }
// The real LogEntry shape from backend/src/server.rs.
const mockLogs: LogEntry[] = [
  { id: '1', event_id: 'e1', pubkey: 'npub1alice', event_kind: 1, timestamp: '2025-01-01T10:00:00Z' }
]
const mockMetrics = { http_requests: 42, nip46_connections: 3, total_signatures: 1234 }

// What each endpoint was asked for, so a role's view can be shown not to request the rest.
let requested: string[] = []

function serve(admin: { logs?: () => unknown } = {}) {
  requested = []
  registerEndpoint('/api/bunker/status', () => {
    requested.push('status')
    return mockStatus
  })
  registerEndpoint('/api/bunker/logs', () => {
    requested.push('logs')
    return admin.logs ? admin.logs() : mockLogs
  })
  registerEndpoint('/api/bunker/metrics', () => {
    requested.push('metrics')
    return mockMetrics
  })
}


describe('Index page', () => {
  beforeEach(async () => {
    await clearNuxtData()
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

  for (const role of ['user', 'signer'] as Role[]) {
    describe(`for a ${role}`, () => {
      beforeEach(() => signInAs(role))

      it('shows the bunker\'s health only, and never requests the administrator\'s data', async () => {
        serve()
        const component = await mountSuspended(Index)

        expect(component.text()).toContain('Bunker Status')
        expect(component.text()).toContain('healthy')
        expect(component.find('[data-testid="role-summary"]').text()).toContain(role === 'user' ? 'User' : 'Signer')
        expect(component.text()).not.toContain('Total Signatures')
        expect(component.text()).not.toContain('Recent Activity')
        expect(requested).toContain('status')
        expect(requested).not.toContain('logs')
        expect(requested).not.toContain('metrics')
      })
    })
  }
})
