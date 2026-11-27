// Which role may open which page (#26, #77): the console's one access matrix. The bunker enforces
// the same matrix server-side (#25, backend/src/proxy_auth.rs `Access`); this decides what the UI
// offers and which routes it refuses, so it is never the only check. A new page registers its route
// here and nothing else needs to change: the sidebar and the middleware both read it.
//
// A department lead (#147) is a signer who leads at least one department: LEAD_ROUTES are the pages
// they may open as well. Each department's own checks are the bunker's, on every request.
import { ROLES } from '#shared/types/bunker'
import type { Role } from '#shared/types/bunker'

const ADMIN: readonly Role[] = ['administrator']
/** Admin is a superset (#77): it keeps the signer's pages. */
const SIGNS: readonly Role[] = ['administrator', 'signer']

/**
 * Each page's route and who may open it. `[param]` matches one path segment, as in Nuxt's page
 * file names. A route not listed here is open (Nuxt's 404 applies).
 */
export const ROUTE_ROLES: Readonly<Record<string, readonly Role[]>> = {
  // Role-aware: each role gets its own dashboard.
  '/': ROLES,
  '/config': ADMIN,
  '/logs': ADMIN,
  // The team and members' profiles are the administrators' alone (#143).
  '/team': ADMIN,
  '/team/[pubkey]': ADMIN,
  // The signer's own Nostr identity and connected apps.
  '/profile': SIGNS,
  '/follows': SIGNS,
  '/relays': SIGNS,
  '/connections': SIGNS,
  // Departments (#147): administrators, and leads (LEAD_ROUTES).
  '/departments': ADMIN,
  // An identity's profile, follows and relays (#150): the same pages, as the identity.
  '/identities/[pubkey]/profile': ADMIN,
  '/identities/[pubkey]/follows': ADMIN,
  '/identities/[pubkey]/relays': ADMIN,
  // Requests waiting for approval (#102): administrators decide, signers see their own.
  '/approvals': SIGNS,
  '/approvals/[id]': SIGNS
}

/**
 * The pages a department lead may open as well as those of their role (#147): the departments they
 * lead, and their identities' profile, follows and relays. Which department is theirs is the
 * bunker's check.
 */
export const LEAD_ROUTES: ReadonlySet<string> = new Set([
  '/departments',
  '/identities/[pubkey]/profile',
  '/identities/[pubkey]/follows',
  '/identities/[pubkey]/relays'
])

/** Page names, for the permission-denied page. */
export const PAGE_TITLES: Readonly<Record<string, string>> = {
  '/': 'the dashboard',
  '/config': 'Config',
  '/team': 'Team',
  '/team/[pubkey]': 'a team member\'s profile',
  '/departments': 'Departments',
  '/logs': 'Logs',
  '/profile': 'your profile',
  '/connections': 'connected apps',
  '/follows': 'follows',
  '/relays': 'default relays',
  '/identities/[pubkey]/profile': 'an identity\'s profile',
  '/identities/[pubkey]/follows': 'an identity\'s follows',
  '/identities/[pubkey]/relays': 'an identity\'s relays',
  '/approvals': 'approvals',
  '/approvals/[id]': 'approvals'
}

function normalise(path: string): string {
  const withoutQuery = path.split(/[?#]/)[0] || '/'
  return withoutQuery === '/' ? '/' : withoutQuery.replace(/\/+$/, '')
}

/** The ROUTE_ROLES key a path falls under: itself, or a pattern with `[param]` segments. */
function routeKey(path: string): string {
  const normalised = normalise(path)
  if (normalised in ROUTE_ROLES) return normalised
  const segments = normalised.split('/')
  const pattern = Object.keys(ROUTE_ROLES).find((key) => {
    const parts = key.split('/')
    return parts.length === segments.length
      && parts.every((part, i) => /^\[[^\]]+\]$/.test(part) ? segments[i] !== '' : part === segments[i])
  })
  return pattern ?? normalised
}

/** Whether `role`, as a department lead when `lead` is set (#147), may open `path`. */
export function canOpen(role: Role, path: string, lead = false): boolean {
  const key = routeKey(path)
  const roles = ROUTE_ROLES[key]
  if (!roles || roles.includes(role)) return true
  return lead && LEAD_ROUTES.has(key)
}

/** Whether a lead could open `path` though their role alone could not (#147). */
export function isLeadRoute(path: string): boolean {
  return LEAD_ROUTES.has(routeKey(path))
}

export function pageTitle(path: string): string {
  return PAGE_TITLES[routeKey(path)] ?? normalise(path)
}

/** Whether a fetch failed because the bunker refused this role, e.g. after a demotion mid-session. */
export function isForbidden(error: unknown): boolean {
  const failure = error as { status?: number, statusCode?: number } | null | undefined
  return (failure?.status ?? failure?.statusCode) === 403
}
