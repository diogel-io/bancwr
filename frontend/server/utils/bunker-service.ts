// Calls the bunker as the frontend itself (the `service` identity, #25), for what sign-in needs
// before anyone is signed in: the bunker's own pubkey (GET /api/bunker/status) and a key lookup
// (GET /api/bunker/team/by-pubkey/:pubkey). The bunker refuses the service identity anything else.
// Sign-in (#11) is the first caller.
import type { H3Event } from 'h3'
import { resolveBackendUrl } from './backend'
import { proxyHeaders, SERVICE_IDENTITY } from './auth/proxy-signature'

export async function bunkerServiceFetch<T>(event: H3Event, path: `/api/bunker/${string}`): Promise<T> {
  const config = useRuntimeConfig(event)
  const headers = config.proxySecret ? proxyHeaders(config.proxySecret, SERVICE_IDENTITY, 'GET', path) : {}
  // An absolute URL to the bunker, not a Nitro route, so Nitro's typed-route inference does not
  // apply; the caller states the response type.
  return await $fetch<T>(`${resolveBackendUrl(config.apiBase)}${path}`, { headers }) as T
}
