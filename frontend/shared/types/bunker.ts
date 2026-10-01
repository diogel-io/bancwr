// Response and request shapes of the bunker API, proxied to the frontend under /api/bunker/*.
// backend/src/server.rs is the source of truth: these mirror its Serialize structs field for
// field, in snake_case as they arrive on the wire. Change both together.

/** Red, yellow or green (#27): any failing check is `unhealthy`, otherwise any warning is `degraded`. */
export type HealthState = 'healthy' | 'degraded' | 'unhealthy'

/** `disabled` is turned off by configuration (NIP-46 off) and does not count. */
export type CheckStatus = 'pass' | 'warn' | 'fail' | 'disabled'

export interface RelayHealth {
  url: string
  connected: boolean
}

/** One of the bunker's health checks: `signer`, `database` or `relays`. */
export interface HealthCheck {
  name: string
  status: CheckStatus
  /** Fixed wording plus relay URLs; never raw errors. */
  detail: string
  /** The relay check only. */
  relays?: RelayHealth[]
}

/** GET /api/bunker/status */
export interface BunkerStatus {
  status: HealthState
  pubkey: string
  /** The bunker's release version, from its BANCWR_VERSION; `0.0.0` when unversioned. */
  version: string
  checks: HealthCheck[]
}

/** GET /api/version — served by the frontend itself, not proxied to the bunker. */
export interface VersionResponse {
  version: string
}

/** GET /api/bunker/logs — one successful signature. The backend logs no failures. */
export interface LogEntry {
  id: string
  event_id: string
  pubkey: string
  event_kind: number
  /** ISO 8601 */
  timestamp: string
}

/** GET /api/bunker/metrics */
export interface Metrics {
  http_requests: number
  nip46_connections: number
  total_signatures: number
}

/** GET /api/bunker/config — read-only; the nsec is never returned. */
export interface ConfigResponse {
  pubkey: string
  nsec_file: string | null
}

/** The three roles (#24). Stored and sent lowercase. */
export type Role = 'administrator' | 'user' | 'signer'

export const ROLE_LABELS: Record<Role, string> = {
  administrator: 'Administrator',
  user: 'User',
  signer: 'Signer'
}

/** GET /api/bunker/team, GET /api/bunker/team/by-pubkey/:pubkey */
export interface TeamMember {
  id: string
  name: string
  /** Canonical lowercase hex. */
  pubkey: string
  /** For display. Null only for a row stored before #24 whose key is not valid. */
  npub: string | null
  /** A Role; a row stored before #24 may hold another value, which grants nothing. */
  role: string
}

/** POST /api/bunker/team. `pubkey` may be an npub or hex. */
export interface AddTeamMemberRequest {
  name: string
  pubkey: string
  role: Role
}

/** POST /api/bunker/team */
export interface TeamOperationResponse {
  success: boolean
  message: string
}
