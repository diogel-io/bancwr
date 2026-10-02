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

/** A NIP-46 connection (#53), from GET /api/bunker/connections. The app signs as the bunker's key. */
export interface Nip46Connection {
  id: string
  /** The app's NIP-46 key (hex). */
  client_pubkey: string
  /** The member it was authorised for (hex). */
  for_pubkey: string
  /** What the app says about itself: never verified. */
  client_name: string | null
  client_url: string | null
  client_image: string | null
  /** Always false: NIP-46 lets an app name itself. */
  metadata_verified: boolean
  /** Event kinds it may sign. */
  kinds: number[]
  connected_at: string
  last_used_at: string | null
  revoked_at: string | null
  /** `logout`, `revoked`, `replaced` or `member_removed`. */
  revoked_reason: string | null
  /** Who revoked it (hex), when someone did (#31). */
  revoked_by: string | null
}

/** A connection token (#53), from GET /api/bunker/connections/tokens: never the secret. */
export interface Nip46Token {
  id: string
  for_pubkey: string
  issued_by: string
  label: string
  kinds: number[]
  created_at: string
  expires_at: string
  used_at: string | null
  revoked_at: string | null
}

/** POST /api/bunker/connections/tokens */
export interface IssueTokenRequest {
  for_pubkey?: string
  label: string
  kinds: number[]
  expires_in_hours?: number
}

/** The token just issued: `uri` carries its secret and is shown once. */
export interface IssueTokenResponse {
  id: string
  uri: string
  for_pubkey: string
  kinds: number[]
  expires_at: string
}
