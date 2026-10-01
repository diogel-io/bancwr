// Which role may open which page (#26). The bunker enforces the same matrix server-side (#25);
// this decides what the UI offers and which routes it refuses, so it is never the only check.
// Pages added later (#30-#33) register their routes here and nothing else needs to change.
import type { Role } from '#shared/types/bunker'

const EVERY_ROLE: readonly Role[] = ['administrator', 'user', 'signer']

/** Each page's route and who may open it. A route not listed here is open (Nuxt's 404 applies). */
export const ROUTE_ROLES: Readonly<Record<string, readonly Role[]>> = {
  // Role-aware: administrators see everything, users and signers the bunker's health only.
  '/': EVERY_ROLE,
  '/config': ['administrator'],
  '/team': ['administrator'],
  '/logs': ['administrator'],
  // Reserved for the pages to come. Signers get their own profile, and nothing else (decided on #26).
  '/profile': EVERY_ROLE,
  '/connections': ['administrator', 'user'],
  '/follows': ['administrator', 'user'],
  '/relays': ['administrator', 'user']
}

/** Page names, for the permission-denied page. */
export const PAGE_TITLES: Readonly<Record<string, string>> = {
  '/': 'the dashboard',
  '/config': 'Config',
  '/team': 'Team',
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

export function canOpen(role: Role, path: string): boolean {
  const roles = ROUTE_ROLES[normalise(path)]
  return roles ? roles.includes(role) : true
}

export function pageTitle(path: string): string {
  return PAGE_TITLES[normalise(path)] ?? normalise(path)
}

/** Whether a fetch failed because the bunker refused this role, e.g. after a demotion mid-session. */
export function isForbidden(error: unknown): boolean {
  const failure = error as { status?: number, statusCode?: number } | null | undefined
  return (failure?.status ?? failure?.statusCode) === 403
}
