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
  /** The primary identity's npub. */
  pubkey: string
  /** Every identity the bunker signs as (#103), hex, the primary first. */
  identities?: string[]
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
  /** The identity that signed (#103), hex. */
  identity_pubkey?: string | null
  /** `console` when an administrator's edit of an identity's profile, follows or relays asked for it (#150), not an app. */
  origin?: 'console' | null
}

/** GET /api/bunker/metrics */
export interface Metrics {
  http_requests: number
  nip46_connections: number
  total_signatures: number
  /** NIP-46 requests refused for a connection's rate limit since the bunker started (#101). */
  nip46_rate_limited: number
  /** NIP-46 requests waiting for an administrator's approval now (#102). */
  approvals_pending: number
}

/** GET /api/bunker/config — read-only; the key is never returned, nor where it is stored. */
export interface ConfigResponse {
  pubkey: string
  /** How the signing key is stored: always `encrypted` since #94. */
  key_storage: 'encrypted'
}

/**
 * The two roles (#77, #143). Stored and sent lowercase, shown as Admin and Signer. Admin is a
 * superset: it signs as a signer does, and also manages the team and the bunker.
 */
export type Role = 'administrator' | 'signer'

/** Every role, in order of access. */
export const ROLES: readonly Role[] = ['administrator', 'signer']

export const ROLE_LABELS: Record<Role, string> = {
  administrator: 'Admin',
  signer: 'Signer'
}

/**
 * Whether a stored role is one of ROLES. Every role signs (#143), so this is also whether the member
 * may hold NIP-46 connections. A row stored before #24 may hold another value, which grants nothing
 * (the bunker's member_cannot_sign).
 */
export function isRole(role: string): role is Role {
  return (ROLES as readonly string[]).includes(role)
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
  /** The identities they may sign as (#103), hex, when the bunker has several; omitted: the primary. */
  identities?: string[]
}

/** PATCH /api/bunker/team/:id (#99). */
export interface ChangeRoleRequest {
  role: Role
}

/** POST /api/bunker/team, DELETE and PATCH /api/bunker/team/:id */
export interface TeamOperationResponse {
  success: boolean
  message: string
}

/** A NIP-46 connection (#53), from GET /api/bunker/connections. The app signs as the bunker's key. */
/** The NIP-46 encryption methods a connection may call (#97), each its own permission. */
export type EncryptionMethod = 'nip44_encrypt' | 'nip44_decrypt' | 'nip04_encrypt' | 'nip04_decrypt'

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
  /** Encryption methods it may call (#97). */
  methods: EncryptionMethod[]
  connected_at: string
  last_used_at: string | null
  revoked_at: string | null
  /** `logout`, `revoked`, `replaced`, `member_removed`, `role_changed` (#77), `restored` (#95), `expired` (#98), `identity_retired` (#146) or `connect_undelivered` (#148). */
  revoked_reason: string | null
  /** Who revoked it (hex), when someone did (#31). */
  revoked_by: string | null
  /** When it stops working (#98); null: until revoked. */
  expires_at: string | null
  /** Its rate limit (#101): requests a minute, and how many may come at once. */
  rate_per_minute: number
  rate_burst: number
  /** Kinds held for an administrator's approval before signing (#102). */
  approval_kinds: number[]
  /** The identity it signs as (#103), hex. */
  identity_pubkey?: string | null
  /** The relays the app chose, when it connected from its own nostrconnect:// code (#148). Empty for a token's connection. */
  app_relays?: string[]
}

/** A connection token (#53), from GET /api/bunker/connections/tokens: never the secret. */
export interface Nip46Token {
  id: string
  for_pubkey: string
  issued_by: string
  label: string
  kinds: number[]
  methods: EncryptionMethod[]
  created_at: string
  expires_at: string
  /** How long its connection lasts once the app connects (#98); null: until revoked. */
  connection_lifetime_hours: number | null
  /** The rate limit of its connection (#101). */
  rate_per_minute: number
  rate_burst: number
  /** Kinds its connection holds for approval (#102). */
  approval_kinds: number[]
  /** The identity its connection signs as (#103), hex. */
  identity_pubkey?: string | null
  used_at: string | null
  revoked_at: string | null
}

/** POST /api/bunker/connections/tokens */
export interface IssueTokenRequest {
  for_pubkey?: string
  label: string
  kinds: number[]
  methods: EncryptionMethod[]
  expires_in_hours?: number
  /** How long the connection lasts once the app connects (#98), 1 to 8760; omitted: until revoked. */
  connection_lifetime_hours?: number
  /** The connection's rate limit (#101): 1 to 600 a minute, burst 1 to 100; omitted: 60, burst 20. */
  rate_per_minute?: number
  rate_burst?: number
  /** Kinds held for approval before signing (#102), each one of `kinds`; omitted: none. */
  approval_kinds?: number[]
  /** The identity the connection signs as (#103), hex; omitted: the primary. */
  identity?: string
}

/** The token just issued: `uri` carries its secret and is shown once. */
export interface IssueTokenResponse {
  id: string
  uri: string
  for_pubkey: string
  kinds: number[]
  methods: EncryptionMethod[]
  expires_at: string
  connection_lifetime_hours: number | null
  rate_per_minute: number
  rate_burst: number
  approval_kinds: number[]
  /** The identity it signs as (#103), hex. */
  identity_pubkey?: string
}

/**
 * GET /api/bunker/identities (#103): an identity this bunker signs as, never its key.
 * Administrators see every identity with its members; others those they may use.
 */
/**
 * A member's role in a department (#147): each identity that isn't administrator only is one
 * department, and a grant of it is a membership. A lead also manages the department.
 */
export type DepartmentRole = 'member' | 'lead'

export interface BunkerIdentity {
  pubkey: string
  npub: string | null
  label: string
  is_primary: boolean
  /** Unlocked in the running bunker: one added since it started loads on restart. */
  available: boolean
  /**
   * Members granted it (hex), leads included: for administrators and the leads of its department.
   * Administrators may use every identity.
   */
  members?: string[]
  /** The leads of its department (#147), hex: listed with `members`. */
  leads?: string[]
  /**
   * Whether grants decide who may use the primary (#103, #147), as the bunker holds it: listed
   * with `members`. False only while everyone who may sign may use the primary.
   */
  grants_matter?: boolean
  /** Belongs to no department (#147): only administrators use it, as the company root does. */
  admin_only?: boolean
  /** The signed-in member's own role in its department, when they hold a grant of it (#147). */
  my_role?: DepartmentRole
  /** What retiring it would revoke (#146), for administrators, on identities that can be retired. */
  usage?: { connections: number, tokens: number }
  /** When it was retired (#146): listed only with `?retired=true`, for administrators. */
  retired_at?: string
}

/**
 * GET /api/bunker/approvals (#102): a `sign_event` waiting for an administrator. The event is shown
 * as it will be signed, under the bunker's key; approving names its `event_id`.
 */
export interface PendingApproval {
  id: string
  member_pubkey: string
  member_name: string | null
  connection_id: string
  client_pubkey: string
  /** What the app calls itself: never verified. */
  client_name: string | null
  /** The identity it would be signed as (#103), hex. Null: the primary. */
  identity_pubkey?: string | null
  kind: number
  content: string
  tags: string[][]
  /** Unix seconds, as the app set it. */
  created_at: number
  event_id: string
  requested_at: string
  expires_at: string
}

/** POST /api/bunker/approvals/:id/approve */
export interface ApproveRequest {
  event_id: string
  note?: string
}

/** POST /api/bunker/approvals/:id/reject */
export interface RejectRequest {
  note?: string
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

/** GET and PUT /api/bunker/logs/retention (#96): how long the signing log and audit events are kept. */
export interface LogRetention {
  /** Rows older than this are pruned; null keeps them indefinitely. */
  max_age_days: number | null
  /** The oldest rows over this many, in each table, are pruned; null for no limit. */
  max_rows: number | null
}

/** The answer to GET and PUT, with the floors a policy may not go under. */
export interface LogRetentionResponse extends LogRetention {
  min_age_days: number
  min_rows: number
}

/** GET /api/bunker/logs/export formats (#96). */
export type LogExportFormat = 'csv' | 'jsonl'

/** What an app's nostrconnect:// code says (#148), read by the bunker. All of it comes from the app and none is verified; never its secret or image. */
export interface NostrConnectPreview {
  /** The app's NIP-46 key (hex). */
  client_pubkey: string
  name: string | null
  url: string | null
  metadata_verified: boolean
  /** The relays it chose, and whether each is one of the bunker's own. */
  relays: { url: string, bunker: boolean }[]
  /** What it asked for: a pre-fill only. `requested_any_kind` is `sign_event` with no kind. */
  requested_kinds: number[]
  requested_any_kind: boolean
  requested_methods: EncryptionMethod[]
  /** Its key already has an active connection, so connecting is refused. */
  already_connected: boolean
}

/** Connecting the app in a nostrconnect:// code (#148): the same choices as a token, without its label or validity. */
export interface NostrConnectRequest {
  uri: string
  for_pubkey?: string
  kinds: number[]
  methods: EncryptionMethod[]
  connection_lifetime_hours?: number
  rate_per_minute?: number
  rate_burst?: number
  approval_kinds?: number[]
  identity?: string
}
