import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import { npubEncode } from 'nostr-tools/nip19'
import ActivityLog from '~/components/ActivityLog.vue'
import type { LogEntry } from '#shared/types/bunker'

const APP = 'a'.repeat(64)
const ALICE = 'b'.repeat(64)
const REMOVED = 'c'.repeat(64)

// The real LogEntry shape from backend/src/server.rs.
const mockLogs: LogEntry[] = [
  { id: '1', event_id: 'e1', pubkey: APP, event_kind: 1, timestamp: '2025-01-01T10:00:00Z', member_pubkey: ALICE, member_name: 'Alice', connection_id: 'c1' },
  { id: '2', event_id: 'e2', pubkey: APP, event_kind: 24133, timestamp: '2025-01-01T11:00:00Z', member_pubkey: REMOVED, member_name: null, connection_id: 'c2' },
  { id: '3', event_id: 'e3', pubkey: APP, event_kind: 7, timestamp: '2025-01-01T12:00:00Z', member_pubkey: null, member_name: null, connection_id: null }
]

const cells = async (rows: LogEntry[]) => {
  const component = await mountSuspended(ActivityLog, { props: { rows } })
  return component.findAll('tbody tr').map(tr => tr.findAll('td').map(td => td.text()))
}

describe('ActivityLog', () => {
  it('shows the member a signature was for, and the app that asked, separately (diogel-io/workspace#38)', async () => {
    const [alice, removed, unknown] = await cells(mockLogs)
    const short = (hex: string) => `${npubEncode(hex).slice(0, 14)}…`

    expect(alice![2]).toBe('Alice')
    expect(alice![3]).toBe(short(APP))
    // A removed member keeps their key; an entry no connection accounts for says so.
    expect(removed![2]).toBe(short(REMOVED))
    expect(unknown![2]).toBe('Unknown')
    expect(unknown![1]).toBe('7')
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
    expect(headers).toEqual(['Timestamp', 'Event Kind', 'Member', 'App'])
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
