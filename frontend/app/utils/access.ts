// Which role may open which page (#26, #77): the console's one access matrix. The bunker enforces
// the same matrix server-side (#25, backend/src/proxy_auth.rs `Access`); this decides what the UI
// offers and which routes it refuses, so it is never the only check. A new page registers its route
// here and nothing else needs to change: the sidebar and the middleware both read it.
import { ROLES } from '#shared/types/bunker'
import type { Role } from '#shared/types/bunker'

const ADMIN: readonly Role[] = ['administrator']
/** Admin is a superset (#77): it keeps the signer's pages and the viewer's. */
const SIGNS: readonly Role[] = ['administrator', 'signer']
const READS_TEAM: readonly Role[] = ['administrator', 'viewer']

/**
 * Each page's route and who may open it. `[param]` matches one path segment, as in Nuxt's page
 * file names. A route not listed here is open (Nuxt's 404 applies).
 */
export const ROUTE_ROLES: Readonly<Record<string, readonly Role[]>> = {
  // Role-aware: each role gets its own dashboard.
  '/': ROLES,
  '/config': ADMIN,
  '/logs': ADMIN,
  // Viewers read the team and members' profiles; only administrators change it.
  '/team': READS_TEAM,
  '/team/[pubkey]': READS_TEAM,
  // The signer's own Nostr identity and connected apps.
  '/profile': SIGNS,
  '/follows': SIGNS,
  '/relays': SIGNS,
  '/connections': SIGNS
}

/** Page names, for the permission-denied page. */
export const PAGE_TITLES: Readonly<Record<string, string>> = {
  '/': 'the dashboard',
  '/config': 'Config',
  '/team': 'Team',
  '/team/[pubkey]': 'a team member\'s profile',
  '/logs': 'Logs',
  '/profile': 'your profile',
  '/connections': 'connected apps',
  '/follows': 'follows',
  '/relays': 'default relays'
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

export function canOpen(role: Role, path: string): boolean {
  const roles = ROUTE_ROLES[routeKey(path)]
  return roles ? roles.includes(role) : true
}

export function pageTitle(path: string): string {
  return PAGE_TITLES[routeKey(path)] ?? normalise(path)
}

/** Whether a fetch failed because the bunker refused this role, e.g. after a demotion mid-session. */
export function isForbidden(error: unknown): boolean {
  const failure = error as { status?: number, statusCode?: number } | null | undefined
  return (failure?.status ?? failure?.statusCode) === 403
}
