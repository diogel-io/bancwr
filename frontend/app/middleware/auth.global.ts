// Every route needs a session (#11), read on each navigation so a removed member or changed role
// takes effect at once. Signed out: /sign-in only. Signed in with a key not in the vault:
// /no-access only. Signed in: not those two. Role-specific routes are #26's.
const PUBLIC_PAGES = new Set(['/sign-in', '/no-access'])

export default defineNuxtRouteMiddleware(async (to) => {
  const state = await useAuth().refresh()

  if (state.status === 'signed-out') {
    return to.path === '/sign-in' ? undefined : navigateTo('/sign-in')
  }
  if (state.status === 'not-registered') {
    return to.path === '/no-access' ? undefined : navigateTo('/no-access')
  }
  return PUBLIC_PAGES.has(to.path) ? navigateTo('/') : undefined
})
