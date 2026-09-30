// The three settings sign-in cannot run without (#11), checked when the server starts
// (server/plugins/auth-config.ts) so a misconfigured deployment fails loudly instead of running
// with sign-in half-working.

export interface AuthConfig {
  /** NUXT_SESSION_PASSWORD: seals the session cookie. */
  sessionPassword: string
  /** NUXT_PROXY_SECRET: signs the caller's identity for the bunker (BANCWR_PROXY_SECRET there). */
  proxySecret: string
  /** NUXT_SITE_ORIGIN: the origin users reach Bancwr on, which sign-in events must name. */
  siteOrigin: string
}

const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1', '[::1]'])

/** Every problem with the configuration, or none. */
export function authConfigProblems(config: Partial<AuthConfig>): string[] {
  const problems: string[] = []
  if (!config.sessionPassword || config.sessionPassword.length < 32) {
    problems.push('NUXT_SESSION_PASSWORD must be set to 32 or more characters (openssl rand -hex 32).')
  }
  if (!config.proxySecret || config.proxySecret.length < 32) {
    problems.push('NUXT_PROXY_SECRET must be set to 32 or more characters, the same value as the bunker\'s BANCWR_PROXY_SECRET.')
  }

  let origin: URL | undefined
  try {
    origin = config.siteOrigin ? new URL(config.siteOrigin) : undefined
  } catch {
    origin = undefined
  }
  if (!origin || origin.origin !== config.siteOrigin) {
    problems.push('NUXT_SITE_ORIGIN must be the origin users reach Bancwr on, such as https://bancwr.example (no path or trailing slash).')
  } else if (origin.protocol !== 'https:' && !(origin.protocol === 'http:' && LOCAL_HOSTS.has(origin.hostname))) {
    // The session cookie is Secure, and browsers only send Secure cookies over HTTPS or to
    // localhost. Over plain HTTP elsewhere, nobody could stay signed in. Decided on #11.
    problems.push('NUXT_SITE_ORIGIN must be https:// (http:// is allowed only for localhost). Put Bancwr behind a TLS reverse proxy; see the README.')
  }
  return problems
}
