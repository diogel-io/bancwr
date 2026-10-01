import { describe, it, expect, beforeEach } from 'vitest'
import { createError } from 'h3'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import type { LogEntry } from '#shared/types/bunker'
import Logs from '../../app/pages/logs.vue'
import { clearNuxtData } from '#imports'

// The real LogEntry shape from backend/src/server.rs.
const mockLogs: LogEntry[] = [
  { id: '1', event_id: 'e1', pubkey: 'npub1alice', event_kind: 1, timestamp: '2025-01-01T10:00:00Z' }
]

describe('Logs page', () => {
  beforeEach(async () => {
    await clearNuxtData()
  })

  it('renders logs page header', async () => {
    registerEndpoint('/api/bunker/logs', {
      method: 'GET',
      handler: () => mockLogs
    })

    const component = await mountSuspended(Logs)

    expect(component.text()).toContain('Signing Activity Log')
    // The data should be passed to ActivityLog
    expect(component.text()).toContain('npub1alice')
  })

  it('handles empty logs', async () => {
    registerEndpoint('/api/bunker/logs', {
      method: 'GET',
      handler: () => []
    })

    const component = await mountSuspended(Logs)
    // It should render ActivityLog which shows "No data"
    expect(component.text()).toContain('No data')
  })

  it('explains a bunker 403 rather than breaking (#26)', async () => {
    registerEndpoint('/api/bunker/logs', () => {
      throw createError({ status: 403, data: { error: 'forbidden' } })
    })

    const component = await mountSuspended(Logs)

    expect(component.find('[data-testid="forbidden-notice"]').text()).toContain('Your role no longer allows this page')
  })
})
