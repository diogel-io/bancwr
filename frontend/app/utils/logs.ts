// The signing log's paging and export (#96). The bunker serves both to administrators:
// GET /api/bunker/logs?limit=&before= and GET /api/bunker/logs/export?format=&from=&to=.
import type { LogEntry, LogExportFormat, LogRetention } from '#shared/types/bunker'

/** Rows per page: the 100 the Logs page always showed. */
export const LOG_PAGE_SIZE = 100

/** The export's default range: this many days, ending today. */
export const DEFAULT_EXPORT_DAYS = 30

/** Where the next page starts: the last row's timestamp and id, as the bunker reads `before`. */
export function nextCursor(rows: readonly LogEntry[]): string | undefined {
  const last = rows.at(-1)
  return last ? `${last.timestamp}|${last.id}` : undefined
}

/** The URL of the page after `rows`. */
export function olderLogsUrl(rows: readonly LogEntry[], limit = LOG_PAGE_SIZE): string {
  const params = new URLSearchParams({ limit: String(limit) })
  const before = nextCursor(rows)
  if (before) params.set('before', before)
  return `/api/bunker/logs?${params}`
}

/** A date as YYYY-MM-DD, in UTC, as the export's range is. */
export function isoDate(date: Date): string {
  return date.toISOString().slice(0, 10)
}

/** The last `days` days, ending on `today`, inclusive. */
export function defaultExportRange(today: Date, days = DEFAULT_EXPORT_DAYS): { from: string, to: string } {
  const from = new Date(today.getTime() - (days - 1) * 24 * 60 * 60 * 1000)
  return { from: isoDate(from), to: isoDate(today) }
}

const DATE = /^\d{4}-\d{2}-\d{2}$/u

/** Why a range cannot be exported, or undefined when it can. */
export function exportRangeError(from: string, to: string): string | undefined {
  if (!DATE.test(from) || !DATE.test(to)) return 'Choose a from and a to date.'
  if (from > to) return 'The from date must not be after the to date.'
  return undefined
}

/** The download URL for a range, both dates included (UTC). */
export function exportUrl(format: LogExportFormat, from: string, to: string): string {
  return `/api/bunker/logs/export?${new URLSearchParams({ format, from, to })}`
}

/** The policy in words, for the Config page. */
export function describeRetention(retention: LogRetention): string {
  const { max_age_days: days, max_rows: rows } = retention
  if (days === null && rows === null) return 'Kept indefinitely'
  const parts: string[] = []
  if (days !== null) parts.push(`${days.toLocaleString('en-GB')} days`)
  if (rows !== null) parts.push(`the newest ${rows.toLocaleString('en-GB')} rows`)
  return `Kept for ${parts.join(', and at most ')}`
}

/**
 * A retention field from the form: empty is null (no limit), anything else a whole number at
 * least `min`. Returns the value, or an error to show.
 */
export function parseRetentionField(value: string | number | null | undefined, min: number, unit: string): { value: number | null } | { error: string } {
  const text = String(value ?? '').trim()
  if (text === '') return { value: null }
  if (!/^\d+$/u.test(text)) return { error: 'Enter a whole number, or leave it empty.' }
  const number = Number(text)
  if (number < min) return { error: `At least ${min.toLocaleString('en-GB')} ${unit}, or leave it empty.` }
  return { value: number }
}
