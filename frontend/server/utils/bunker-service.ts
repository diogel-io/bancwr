// Calls the bunker as the frontend itself (the `service` identity, #25), for what sign-in needs
// before anyone is signed in: the bunker's own pubkey (GET /api/bunker/status) and a key lookup
// (GET /api/bunker/team/by-pubkey/:pubkey). The bunker refuses the service identity anything else.
// Sign-in (#11) is the first caller.
//
// SPIKE (#151): to any of the configured bunkers, signed with that bunker's own secret, and also
// the member-proof routes (POST /api/bunker/auth/challenge and /proof).
import type { H3Event } from 'h3'
import { proxyHeaders, SERVICE_IDENTITY } from './auth/proxy-signature'
import { defaultBunker, type BunkerTarget } from './bunkers'

export async function bunkerServiceFetch<T>(
  event: H3Event,
  path: `/api/bunker/${string}`,
  options: { bunker?: BunkerTarget, method?: 'GET' | 'POST', body?: unknown } = {}
): Promise<T> {
  const config = useRuntimeConfig(event)
  const bunker = options.bunker ?? defaultBunker(config)
  const method = options.method ?? 'GET'
  const body = options.body === undefined ? '' : JSON.stringify(options.body)
  const headers = bunker.secret ? proxyHeaders(bunker.secret, SERVICE_IDENTITY, method, path, body) : {}
  // An absolute URL to the bunker, not a Nitro route, so Nitro's typed-route inference does not
  // apply; the caller states the response type.
  return await $fetch<T>(`${bunker.origin}${path}`, {
    method,
    headers: body ? { ...headers, 'Content-Type': 'application/json' } : headers,
    body: body || undefined
  }) as T
}
