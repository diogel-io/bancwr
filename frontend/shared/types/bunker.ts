// Response and request shapes of the bunker API, proxied to the frontend under /api/bunker/*.
// backend/src/server.rs is the source of truth: these mirror its Serialize structs field for
// field, in snake_case as they arrive on the wire. Change both together.

/** GET /api/bunker/status */
export interface BunkerStatus {
  status: string
  pubkey: string
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

/** GET /api/bunker/config — the nsec is never returned. */
export interface ConfigResponse {
  pubkey: string
  nsec_file: string | null
}

/** POST /api/bunker/config — set one of nsec or nsec_file, not both. */
export interface ConfigUpdateRequest {
  nsec?: string
  nsec_file?: string
}

/** POST /api/bunker/config */
export interface ConfigUpdateResponse {
  success: boolean
  message: string
  pubkey: string | null
}

/** GET /api/bunker/team */
export interface TeamMember {
  id: string
  name: string
  pubkey: string
  role: string
}

/** POST /api/bunker/team */
export interface AddTeamMemberRequest {
  name: string
  pubkey: string
  role: string
}

/** POST /api/bunker/team */
export interface TeamOperationResponse {
  success: boolean
  message: string
}
