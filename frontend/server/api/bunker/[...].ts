// Nitro server route to proxy all /api/bunker/* to backend
// Imported rather than left to Nitro's auto-import: nothing in this file's type context
// resolves auto-imports, so an explicit path keeps our own symbols checkable.
import { resolveBackendUrl, BackendTargetError } from '../../utils/backend'

export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig()

  // No inline fallback: runtimeConfig already supplies the default, and two of them hide which
  // one is in force. resolveBackendUrl refuses a target that is this server — see #20. Its own
  // try/catch, because a misconfigured target is ours to explain, not a backend failure to
  // forward, and the catch below is shaped for $fetch errors.
  let backendUrl: string
  try {
    backendUrl = resolveBackendUrl(config.apiBase, {
      requestOrigin: getRequestURL(event).origin,
      selfPort: process.env.PORT,
    })
  } catch (error) {
    if (error instanceof BackendTargetError) {
      throw createError({ statusCode: 500, statusMessage: error.message })
    }

    throw error
  }

  // Get the path after /api/bunker/
  const path = event.context.params?._ || ''
  const url = `${backendUrl}/api/bunker/${path}`

  // Forward the request
  const method = getMethod(event)
  const body = method !== 'GET' ? await readBody(event) : undefined
  const query = getQuery(event)

  try {
    return await $fetch(url, {
      method,
      body,
      query,
      headers: {
        // Forward relevant headers
        'Content-Type': 'application/json',
      },
    })
  } catch (error: any) {
    // Forward error response
    throw createError({
      statusCode: error.response?.status || 500,
      statusMessage: error.response?.statusText || 'Internal Server Error',
      data: error.response?._data,
    })
  }
})
