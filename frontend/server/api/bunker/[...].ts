// Nitro server route to proxy all /api/bunker/* to the default bunker: the only one, without
// NUXT_BUNKERS. The path is forwarded as it arrived, and the signature covers that exact string.
// SPIKE (#151): the proxying itself is in server/utils/bunker-proxy.ts, shared with
// /api/bunkers/<id>/*.
import { proxyToBunker } from '../../utils/bunker-proxy'
import { defaultBunker } from '../../utils/bunkers'

export default defineEventHandler(async (event) => {
  const { pathname, search } = getRequestURL(event)
  return await proxyToBunker(event, defaultBunker(useRuntimeConfig(event)), `${pathname}${search}`)
})
