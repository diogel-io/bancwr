// SPIKE (#23).
import { useBancwrSession } from '../../utils/auth/session'

export default defineEventHandler(async (event) => {
  const session = await useBancwrSession(event, useRuntimeConfig(event).sessionPassword)
  await session.clear()
  return { ok: true }
})
