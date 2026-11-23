// The bunker's refusal, `{ error, message }`, from a failed `$fetch` to `/api/bunker/*`.
//
// The proxy (`server/api/bunker/[...].ts`) rethrows a refusal as an H3 error, so the browser
// receives Nuxt's error body: `{ statusCode, statusMessage, message, data }`, with the bunker's own
// answer under `data`. Its top-level `message` is only the HTTP status text, such as "Conflict",
// and must never be shown as the reason. A body that is not wrapped, as in a test endpoint that
// returns the bunker's answer directly, is read as it is.

export interface BunkerRefusal {
  error?: string
  message?: string
}

type Body = BunkerRefusal & { statusCode?: number, data?: unknown }

export function bunkerRefusal(failure: unknown): BunkerRefusal {
  const body = (failure as { data?: Body } | undefined)?.data
  if (!body || typeof body !== 'object') return {}
  if (body.data && typeof body.data === 'object') {
    const { error, message } = body.data as BunkerRefusal
    return { error, message }
  }
  // Nuxt's error body with no answer from the bunker: only the status text, nothing to show.
  if (typeof body.statusCode === 'number') return {}
  return { error: body.error, message: body.message }
}
