// POST /api/auth/logout: ends the session, including any copy of its cookie.
import { endSession } from '../../utils/auth/session'

export default defineEventHandler(async (event) => {
  await endSession(event)
  return { ok: true }
})
