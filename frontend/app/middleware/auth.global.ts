// Every route needs a session (#11), read on each navigation so a removed member or changed role
// takes effect at once. Signed out: /sign-in only. Signed in with a key not in the vault:
// /no-access only. Signed in: not those two, and only the routes the role may open (#26), which
// are refused in place with a 403 rather than redirected, so a typed URL cannot loop. A department
// lead's pages (#147) are read from the bunker's identities on each such navigation.
import type { BunkerIdentity } from '#shared/types/bunker'
import { canOpen, isLeadRoute } from '~/utils/access'

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
  // A page only a department lead may open as well (#147): read whether they lead one now.
  const lead = !canOpen(state.role, to.path) && isLeadRoute(to.path) && await leadsADepartment()
  if (!canOpen(state.role, to.path, lead)) {
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

/** Whether the signed-in member leads any department (#147), as the bunker lists it now. */
async function leadsADepartment(): Promise<boolean> {
  try {
    const identities = await $fetch<BunkerIdentity[]>('/api/bunker/identities')
    return identities.some(identity => identity.my_role === 'lead')
  } catch {
    return false
  }
}
