// SPIKE (#151): the bunkers this console manages. NUXT_BUNKERS is a JSON list, one entry per bunker:
//
//   [{"id":"main","label":"Bancwr","origin":"http://bunker:3000","secretFile":"/run/secrets/main_proxy"},
//    {"id":"root","label":"Company root","origin":"http://bunker-root:3000","secretFile":"/run/secrets/root_proxy"}]
//
// Each bunker has its own proxy secret, read from a file, so a secret leaked from one bunker's host
// signs nothing another bunker accepts. Without NUXT_BUNKERS, the console has one bunker, "main",
// at NUXT_API_BASE with NUXT_PROXY_SECRET: a single-bunker deployment needs no change.
//
// The first bunker is the default: /api/bunker/* still reaches it, and the console's own sign-in
// questions (role, no administrator) are asked of it.
import { readFileSync } from 'node:fs'
import { resolveBackendUrl } from './backend'

export interface BunkerTarget {
  /** Lowercase letters, digits and hyphens: the path segment in /api/bunkers/<id>/…. */
  id: string
  label: string
  origin: string
  secret: string
}

const ID = /^[a-z0-9][a-z0-9-]{0,31}$/

interface Entry {
  id?: unknown
  label?: unknown
  origin?: unknown
  secretFile?: unknown
}

let cache: { key: string, bunkers: BunkerTarget[] } | undefined

/** Every bunker, the default first. Throws on a list that is not valid, naming what is wrong. */
export function configuredBunkers(config: { apiBase: string, proxySecret: string, bunkers?: unknown }): BunkerTarget[] {
  const key = `${JSON.stringify(config.bunkers ?? '')}\n${config.apiBase}\n${config.proxySecret}`
  if (cache?.key === key) return cache.bunkers

  let bunkers: BunkerTarget[]
  if (!config.bunkers) {
    bunkers = [{ id: 'main', label: 'Bancwr', origin: resolveBackendUrl(config.apiBase), secret: config.proxySecret }]
  } else {
    // Nuxt parses a JSON environment value itself, so NUXT_BUNKERS may arrive as the list already.
    const entries = (typeof config.bunkers === 'string' ? JSON.parse(config.bunkers) : config.bunkers) as Entry[]
    if (!Array.isArray(entries) || entries.length === 0) throw new Error('NUXT_BUNKERS must be a non-empty JSON list')
    const seen = new Set<string>()
    bunkers = entries.map((entry, index) => {
      if (typeof entry.id !== 'string' || !ID.test(entry.id)) throw new Error(`NUXT_BUNKERS[${index}].id must be lowercase letters, digits and hyphens`)
      if (seen.has(entry.id)) throw new Error(`NUXT_BUNKERS names the bunker "${entry.id}" twice`)
      seen.add(entry.id)
      if (typeof entry.secretFile !== 'string') throw new Error(`NUXT_BUNKERS[${index}].secretFile must name the file holding that bunker's proxy secret`)
      const secret = readFileSync(entry.secretFile, 'utf8').trim()
      if (secret.length < 32) throw new Error(`The proxy secret for bunker "${entry.id}" must be 32 or more characters`)
      return {
        id: entry.id,
        label: typeof entry.label === 'string' && entry.label ? entry.label : entry.id,
        origin: resolveBackendUrl(String(entry.origin)),
        secret
      }
    })
    const secrets = new Set(bunkers.map(b => b.secret))
    if (secrets.size !== bunkers.length) throw new Error('Each bunker in NUXT_BUNKERS needs a proxy secret of its own')
  }
  cache = { key, bunkers }
  return bunkers
}

export function defaultBunker(config: Parameters<typeof configuredBunkers>[0]): BunkerTarget {
  return configuredBunkers(config)[0]!
}

export function findBunker(config: Parameters<typeof configuredBunkers>[0], id: string): BunkerTarget | undefined {
  return configuredBunkers(config).find(b => b.id === id)
}
