import { describe, it, expect } from 'vitest'
import { downloadHeaders, isDownload } from '../../server/utils/download'

describe('proxy downloads (#96)', () => {
  it('streams only the log export', () => {
    expect(isDownload('/api/bunker/logs/export')).toBe(true)
    expect(isDownload('/api/bunker/logs')).toBe(false)
    expect(isDownload('/api/bunker/logs/export/')).toBe(false)
  })

  it('keeps only the type and file name from the bunker, and adds no-store', () => {
    const fromBunker = new Headers({
      'content-type': 'text/csv; charset=utf-8',
      'content-disposition': 'attachment; filename="bancwr-log-2026-06-01-2026-06-30.csv"',
      'set-cookie': 'session=stolen',
      'x-bancwr-identity': 'aa',
      'access-control-allow-origin': '*'
    })
    expect(downloadHeaders(fromBunker)).toEqual({
      'cache-control': 'no-store',
      'content-type': 'text/csv; charset=utf-8',
      'content-disposition': 'attachment; filename="bancwr-log-2026-06-01-2026-06-30.csv"'
    })
  })
})
