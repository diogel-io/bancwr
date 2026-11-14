import { describe, it, expect } from 'vitest'
import type { LogEntry } from '#shared/types/bunker'
import {
  LOG_PAGE_SIZE,
  defaultExportRange,
  describeRetention,
  exportRangeError,
  exportUrl,
  nextCursor,
  olderLogsUrl,
  parseRetentionField
} from '../../app/utils/logs'

const entry = (id: string, timestamp: string): LogEntry => ({ id, event_id: `e${id}`, pubkey: 'cc', event_kind: 1, timestamp })

describe('log paging (#96)', () => {
  it('continues from the last row, as the bunker reads before', () => {
    const rows = [entry('a', '2026-06-01T12:00:02+00:00'), entry('b', '2026-06-01T12:00:01.5+00:00')]
    expect(nextCursor(rows)).toBe('2026-06-01T12:00:01.5+00:00|b')
    expect(nextCursor([])).toBeUndefined()
  })

  it('asks for the next page with the cursor encoded', () => {
    const url = new URL(olderLogsUrl([entry('b', '2026-06-01T12:00:01+00:00')]), 'http://x')
    expect(url.pathname).toBe('/api/bunker/logs')
    expect(url.searchParams.get('limit')).toBe(String(LOG_PAGE_SIZE))
    expect(url.searchParams.get('before')).toBe('2026-06-01T12:00:01+00:00|b')
    expect(olderLogsUrl([])).toBe(`/api/bunker/logs?limit=${LOG_PAGE_SIZE}`)
  })
})

describe('log export (#96)', () => {
  it('defaults to the last 30 days, today included', () => {
    expect(defaultExportRange(new Date('2026-06-30T23:00:00Z'))).toEqual({ from: '2026-06-01', to: '2026-06-30' })
  })

  it('refuses a missing or reversed range', () => {
    expect(exportRangeError('2026-06-01', '2026-06-01')).toBeUndefined()
    expect(exportRangeError('', '2026-06-01')).toBe('Choose a from and a to date.')
    expect(exportRangeError('2026-06-02', '2026-06-01')).toBe('The from date must not be after the to date.')
  })

  it('builds the download URL', () => {
    expect(exportUrl('jsonl', '2026-06-01', '2026-06-30')).toBe('/api/bunker/logs/export?format=jsonl&from=2026-06-01&to=2026-06-30')
  })
})

describe('log retention (#96)', () => {
  it('says the policy in words', () => {
    expect(describeRetention({ max_age_days: null, max_rows: null })).toBe('Kept indefinitely')
    expect(describeRetention({ max_age_days: 90, max_rows: null })).toBe('Kept for 90 days')
    expect(describeRetention({ max_age_days: 365, max_rows: 50000 })).toBe('Kept for 365 days, and at most the newest 50,000 rows')
    expect(describeRetention({ max_age_days: null, max_rows: 10000 })).toBe('Kept for the newest 10,000 rows')
  })

  it('reads a field: empty is no limit, and the floor is enforced', () => {
    expect(parseRetentionField('', 30, 'days')).toEqual({ value: null })
    expect(parseRetentionField(' 90 ', 30, 'days')).toEqual({ value: 90 })
    expect(parseRetentionField('29', 30, 'days')).toEqual({ error: 'At least 30 days, or leave it empty.' })
    expect(parseRetentionField('9999', 10000, 'rows')).toEqual({ error: 'At least 10,000 rows, or leave it empty.' })
    expect(parseRetentionField('1.5', 30, 'days')).toEqual({ error: 'Enter a whole number, or leave it empty.' })
    expect(parseRetentionField('-40', 30, 'days')).toEqual({ error: 'Enter a whole number, or leave it empty.' })
  })
})
