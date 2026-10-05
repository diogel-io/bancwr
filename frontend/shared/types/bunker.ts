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

/**
 * GET /api/bunker/logs — one successful signature. The backend logs no failures. Also
 * GET /api/bunker/logs/mine (#77): the caller's own, for the signer dashboard.
 */
export interface LogEntry {
  id: string
  event_id: string
  /** The key that asked for the signature: the NIP-46 app's (hex), not the member's. */
  pubkey: string
  event_kind: number
  /** ISO 8601 */
  timestamp: string
  /** The vault member the app's connection was made for (hex); null when not known (diogel-io/workspace#38). */
  member_pubkey?: string | null
  /** That member's name, while they are still in the vault. */
  member_name?: string | null
  connection_id?: string | null
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

/**
 * The three roles (#77, which replaced #24's administrator, user and signer). Stored and sent
 * lowercase, shown as Admin, Signer and Viewer. Admin is a superset; a viewer reads the team and
 * members' profiles and never holds a NIP-46 connection.
 */
export type Role = 'administrator' | 'signer' | 'viewer'

/** Every role, in order of access. */
export const ROLES: readonly Role[] = ['administrator', 'signer', 'viewer']

export const ROLE_LABELS: Record<Role, string> = {
  administrator: 'Admin',
  signer: 'Signer',
  viewer: 'Viewer'
}

/** Whether a member with this role may hold NIP-46 connections (#77); backend `Role::can_sign`. */
export function canSign(role: string): boolean {
  return role === 'administrator' || role === 'signer'
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
  /** `logout`, `revoked`, `replaced`, `member_removed` or `role_changed` (#77). */
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

/** Where the bunker's own relays come from (#78): `environment` is NIP46_RELAYS, read-only here. */
export type RelaySource = 'environment' | 'console'

/** GET /api/bunker/relays, and the answer to PUT (administrators). */
export interface BunkerRelays {
  source: RelaySource
  /** NIP46_ENABLED: while on, a saved list needs at least one relay. */
  nip46_enabled: boolean
  /** In order, as they appear in a bunker:// string, with whether each is connected now. */
  relays: RelayHealth[]
}

/** PUT /api/bunker/relays: the whole list, replacing the one saved. */
export interface ReplaceBunkerRelaysRequest {
  relays: string[]
}

/** The bunker's relay limits (#78), as backend/src/bunker_relays.rs enforces them. */
export const MAX_BUNKER_RELAYS = 6
/** Fewer than this is allowed, but every connected app then depends on one relay. */
export const ADVISED_MIN_BUNKER_RELAYS = 2
