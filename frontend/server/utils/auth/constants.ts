// SPIKE (diogel-io/bancwr#23). Values the decision record fixes; #11 and #25 take them from there.

/** NIP-98 HTTP Auth. The login request is authorised by one of these, never published. */
export const LOGIN_EVENT_KIND = 27235

/** How long an issued challenge may be used. Covers a NIP-46 signer's `auth_url` approval. */
export const CHALLENGE_TTL_SECONDS = 300

/** How far `created_at` may be from the server clock. NIP-98 suggests 60 s; see the ADR. */
export const CREATED_AT_PAST_SECONDS = 120
export const CREATED_AT_FUTURE_SECONDS = 30

/** Session lifetime. The role is re-read on every request, so this bounds identity, not access. */
export const SESSION_MAX_AGE_SECONDS = 12 * 60 * 60

/** The path of the login route, which the `u` tag must name exactly, under the public origin. */
export const LOGIN_PATH = '/api/auth/login'
