// Display helpers for connected apps (#31).
import type { EncryptionMethod } from '#shared/types/bunker'

/** Names for the event kinds an app is likely to be granted (NIP-01 and the NIPs that define them). */
export const KIND_NAMES: Record<number, string> = {
  0: 'Profile',
  1: 'Note',
  3: 'Follows',
  4: 'Encrypted DM',
  5: 'Deletion',
  6: 'Repost',
  7: 'Reaction',
  16: 'Generic repost',
  1111: 'Comment',
  9734: 'Zap request',
  10002: 'Relay list',
  22242: 'Relay auth',
  24242: 'Blossom auth',
  27235: 'HTTP auth',
  30023: 'Article'
}

export function kindLabel(kind: number): string {
  return KIND_NAMES[kind] ? `${kind} ${KIND_NAMES[kind]}` : String(kind)
}

/** Presets for a new token: what an app of that sort needs. */
export const KIND_PRESETS: { label: string, kinds: number[] }[] = [
  { label: 'Notes and reactions', kinds: [1, 7] },
  { label: 'Notes, reactions and reposts', kinds: [1, 6, 7] },
  { label: 'Profile only', kinds: [0] }
]

/** The encryption methods an app may be allowed (#97), in the order the bunker lists them. */
export const ENCRYPTION_METHODS: { value: EncryptionMethod, label: string }[] = [
  { value: 'nip44_encrypt', label: 'Encrypt messages (NIP-44)' },
  { value: 'nip44_decrypt', label: 'Read encrypted messages (NIP-44)' },
  { value: 'nip04_encrypt', label: 'Encrypt messages (NIP-04, older apps)' },
  { value: 'nip04_decrypt', label: 'Read encrypted messages (NIP-04, older apps)' }
]

export function methodLabel(method: EncryptionMethod): string {
  return ENCRYPTION_METHODS.find(m => m.value === method)?.label ?? method
}

/** Whether a method lets the app read what was sent to the brand's key. */
export function isDecrypt(method: EncryptionMethod): boolean {
  return method.endsWith('_decrypt')
}

/** Custom kinds from "1, 7, 30023": numbers 0 to 65535, or why not. */
export function parseKinds(input: string): { kinds: number[] } | { error: string } {
  const parts = input.split(/[\s,]+/u).filter(Boolean)
  const kinds: number[] = []
  for (const part of parts) {
    if (!/^\d+$/u.test(part) || Number(part) > 65535) return { error: `"${part}" is not a kind: use numbers from 0 to 65535.` }
    if (!kinds.includes(Number(part))) kinds.push(Number(part))
  }
  return kinds.length ? { kinds: kinds.sort((a, b) => a - b) } : { error: 'Choose at least one kind.' }
}

/** How long a connection may last once the app connects (#98, #149). 0 is until revoked. */
export const LIFETIMES: { label: string, value: number }[] = [
  { label: 'Until revoked', value: 0 },
  { label: '1 hour', value: 1 },
  { label: '8 hours', value: 8 },
  { label: '1 day', value: 24 },
  { label: '7 days', value: 168 },
  { label: '30 days', value: 720 },
  { label: '90 days', value: 2160 }
]

/** "1 day", "30 days", "36 hours": a connection lifetime in hours, in whole days when it is. */
export function lifetimeLabel(hours: number): string {
  if (hours % 24 === 0) return `${hours / 24} day${hours === 24 ? '' : 's'}`
  return `${hours} hour${hours === 1 ? '' : 's'}`
}

/** The bunker's default rate limit (#101), used when a token is issued without one. */
export const DEFAULT_RATE_LIMIT = { perMinute: 60, burst: 20 }

/** Rate limits for a new token (#101). The first is the default, which sends nothing. */
export const RATE_LIMITS: { label: string, value: number, perMinute: number, burst: number }[] = [
  { label: 'Default: 60 a minute (burst 20)', value: 0, ...DEFAULT_RATE_LIMIT },
  { label: '10 a minute (burst 5)', value: 1, perMinute: 10, burst: 5 },
  { label: '30 a minute (burst 10)', value: 2, perMinute: 30, burst: 10 },
  { label: '120 a minute (burst 40)', value: 3, perMinute: 120, burst: 40 },
  { label: '600 a minute (burst 100)', value: 4, perMinute: 600, burst: 100 }
]

/** "60 a minute (burst 20)": a connection's rate limit (#101). */
export function rateLimitLabel(perMinute: number, burst: number): string {
  return `${perMinute} a minute (burst ${burst})`
}

/** When an active connection stops working (#98): "ends in 6 days", or that it has no limit. */
export function connectionEnd(expiresAt: string | null, now = Date.now()): string {
  if (!expiresAt) return 'no time limit'
  const ended = new Date(expiresAt).getTime() <= now
  return `${ended ? 'time limit reached' : 'ends'} ${relativeTime(expiresAt, now)}`
}

/** "3 minutes ago", "2 days ago", from an ISO time, relative to `now`. */
export function relativeTime(iso: string, now = Date.now()): string {
  const seconds = Math.round((now - new Date(iso).getTime()) / 1000)
  const future = seconds < 0
  const s = Math.abs(seconds)
  const [value, unit] = s < 60 ? [s, 'second'] : s < 3600 ? [Math.round(s / 60), 'minute'] : s < 86400 ? [Math.round(s / 3600), 'hour'] : [Math.round(s / 86400), 'day']
  if (unit === 'second' && value < 10) return future ? 'in a moment' : 'just now'
  const text = `${value} ${unit}${value === 1 ? '' : 's'}`
  return future ? `in ${text}` : `${text} ago`
}

const REASONS: Record<string, string> = {
  logout: 'The app logged out',
  revoked: 'Revoked',
  replaced: 'Replaced by a newer connection from the same app',
  member_removed: 'Its member was removed from the vault',
  role_changed: 'Its member\'s role was changed so they could no longer sign',
  restored: 'The bunker was restored from a backup',
  expired: 'Its time limit ran out',
  identity_retired: 'The identity it signed as was retired',
  connect_undelivered: 'None of the app\'s relays took the bunker\'s answer'
}

export function revokedReason(reason: string | null): string {
  return (reason && REASONS[reason]) ?? 'Ended'
}
