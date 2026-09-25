// Resolving the bunker's address is worth isolating: getting it wrong once shipped a frontend
// that proxied to itself. `compose.yaml` set NUXT_API_PROXY_TARGET, which nothing read, so the
// container fell back to http://localhost:3000 — its own listener, because the image sets
// PORT=3000 — and every /api/bunker/* request re-entered this route instead of reaching the
// bunker. See diogel-io/bancwr#20.

const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]', '::1'])
const DEFAULT_PORTS: Record<string, string> = { 'http:': '80', 'https:': '443' }

export class BackendTargetError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'BackendTargetError'
  }
}

/** Host and port, with the protocol's default port made explicit so :80 and "" compare equal. */
function authority(url: URL): string {
  return `${url.hostname}:${url.port || DEFAULT_PORTS[url.protocol] || ''}`
}

function parse(value: string, label: string): URL {
  const invalid = () =>
    new BackendTargetError(
      `${label} is not a valid http(s) origin: ${JSON.stringify(value)}. ` +
        'Set NUXT_API_BASE to the bunker origin, for example http://bunker:3000.'
    )

  let url: URL
  try {
    url = new URL(value)
  } catch {
    throw invalid()
  }

  // `new URL` is more permissive than it looks: "bunker:3000" parses happily as scheme
  // "bunker:" with path "3000", and its origin is the string "null". Require a real http(s)
  // origin so a value like that fails here rather than downstream.
  if ((url.protocol !== 'http:' && url.protocol !== 'https:') || !url.hostname) {
    throw invalid()
  }

  return url
}

export interface ResolveOptions {
  /** Origin this request arrived on, if known. Catches a target aimed back at us. */
  requestOrigin?: string | null
  /** Port this process listens on, if known. Catches a loopback target aimed at ourselves. */
  selfPort?: string | number | null
}

/**
 * Returns the origin to proxy bunker requests to, or throws when that origin is this very
 * server. Proxying to ourselves is never what anyone meant, and left unchecked it recurses
 * rather than failing.
 */
export function resolveBackendUrl(configuredBase: string, options: ResolveOptions = {}): string {
  const base = parse(configuredBase, 'NUXT_API_BASE')
  const target = authority(base)

  const selfPort = options.selfPort == null ? null : String(options.selfPort)
  if (selfPort && LOOPBACK.has(base.hostname) && (base.port || DEFAULT_PORTS[base.protocol]) === selfPort) {
    throw new BackendTargetError(
      `NUXT_API_BASE points at this server itself (${configuredBase}, and this process listens ` +
        `on port ${selfPort}). Set it to the bunker's address — under compose that is ` +
        'http://bunker:3000.'
    )
  }

  if (options.requestOrigin) {
    // A bad Host header only makes this miss the check, never invent one, so a parse failure
    // here is not worth failing the request over.
    try {
      if (authority(new URL(options.requestOrigin)) === target) {
        throw new BackendTargetError(
          `NUXT_API_BASE points back at the origin this request arrived on (${target}), so ` +
            'proxying it would re-enter this route. Set it to the bunker\'s address — under ' +
            'compose that is http://bunker:3000.'
        )
      }
    } catch (error) {
      if (error instanceof BackendTargetError) throw error
    }
  }

  return base.origin
}
