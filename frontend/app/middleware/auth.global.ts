// Every route needs a session (#11), read on each navigation so a removed member or changed role
// takes effect at once. Signed out: /sign-in only. Signed in with a key not in the vault:
// /no-access only. Signed in: not those two, and only the routes the role may open (#26), which
// are refused in place with a 403 rather than redirected, so a typed URL cannot loop.
import { canOpen } from '~/utils/access'

const PUBLIC_PAGES = new Set(['/sign-in', '/no-access'])

export default defineNuxtRouteMiddleware(async (to) => {
  const state = await useAuth().refresh()

  if (state.status === 'signed-out') {
    return to.path === '/sign-in' ? undefined : navigateTo('/sign-in')
  }
  if (state.status === 'not-registered') {
    return to.path === '/no-access' ? undefined : navigateTo('/no-access')
  }
  if (PUBLIC_PAGES.has(to.path)) {
    return navigateTo('/')
  }
  if (!canOpen(state.role, to.path)) {
    // Fatal, or a client-side navigation is only cancelled and the user is left wondering why.
    return abortNavigation(createError({
      status: 403,
      statusText: 'Forbidden',
      fatal: true,
      data: { reason: 'forbidden_route', path: to.path }
    }))
  }
  return undefined
})
