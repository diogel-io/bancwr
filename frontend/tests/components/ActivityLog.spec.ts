import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import ActivityLog from '~/components/ActivityLog.vue'
import type { LogEntry } from '#shared/types/bunker'

// The real LogEntry shape from backend/src/server.rs.
const mockLogs: LogEntry[] = [
  { id: '1', event_id: 'e1', pubkey: 'npub1alice', event_kind: 1, timestamp: '2025-01-01T10:00:00Z' },
  { id: '2', event_id: 'e2', pubkey: 'npub1bob', event_kind: 24133, timestamp: '2025-01-01T11:00:00Z' }
]

describe('ActivityLog', () => {
  it('renders the signer pubkey and event kind of each entry', async () => {
    const component = await mountSuspended(ActivityLog, {
      props: {
        rows: mockLogs
      }
    })

    expect(component.text()).toContain('npub1alice')
    expect(component.text()).toContain('npub1bob')
    expect(component.text()).toContain('24133')
  })

  it('formats the timestamp from the entry', async () => {
    const component = await mountSuspended(ActivityLog, {
      props: {
        rows: mockLogs
      }
    })

    expect(component.text()).toContain(new Date(mockLogs[0]!.timestamp).toLocaleString())
    expect(component.text()).not.toContain('Invalid Date')
  })

  it('has no status column, because the backend logs successful signatures only', async () => {
    const component = await mountSuspended(ActivityLog, {
      props: {
        rows: mockLogs
      }
    })

    const headers = component.findAll('th').map(th => th.text())
    expect(headers).toEqual(['Timestamp', 'Event Kind', 'Member'])
  })

  it('handles empty state', async () => {
    const component = await mountSuspended(ActivityLog, {
      props: {
        rows: []
      }
    })
    expect(component.text()).toContain('No data')
  })
})
