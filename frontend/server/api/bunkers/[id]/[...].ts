// SPIKE (#151): /api/bunkers/<id>/<rest> reaches the bunker <id> as /api/bunker/<rest>, so each
// bunker's routes, its access matrix and the path its signature covers stay as they are.
import { proxyToBunker } from '../../../utils/bunker-proxy'
import { findBunker } from '../../../utils/bunkers'

export default defineEventHandler(async (event) => {
  const id = getRouterParam(event, 'id') ?? ''
  const bunker = findBunker(useRuntimeConfig(event), id)
  if (!bunker) throw createError({ statusCode: 404, statusMessage: 'No such bunker', data: { error: 'unknown_bunker' } })
  const { pathname, search } = getRequestURL(event)
  const rest = pathname.slice(`/api/bunkers/${id}`.length)
  return await proxyToBunker(event, bunker, `/api/bunker${rest}${search}`)
})
