// @vitest-environment node
// SPIKE (#151): the bunkers a console manages. Without NUXT_BUNKERS it is the one bunker it always
// had; with it, each bunker has its own origin and a proxy secret of its own.
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { configuredBunkers, findBunker } from '../../server/utils/bunkers'

const MAIN = 'm'.repeat(40)
const ROOT = 'r'.repeat(40)

function secretFile(value: string) {
  const file = join(mkdtempSync(join(tmpdir(), 'bunkers-')), 'secret')
  writeFileSync(file, `${value}\n`)
  return file
}

describe('configuredBunkers', () => {
  it('is the single bunker it always was without NUXT_BUNKERS', () => {
    expect(configuredBunkers({ apiBase: 'http://bunker:3000', proxySecret: MAIN })).toEqual([
      { id: 'main', label: 'Bancwr', origin: 'http://bunker:3000', secret: MAIN }
    ])
  })

  it('reads each bunker with its own secret, the first the default', () => {
    const bunkers = JSON.stringify([
      { id: 'main', label: 'Bancwr', origin: 'http://bunker:3000', secretFile: secretFile(MAIN) },
      { id: 'root', label: 'Company root', origin: 'http://bunker-root:3000', secretFile: secretFile(ROOT) }
    ])
    const config = { apiBase: 'http://ignored:3000', proxySecret: '', bunkers }
    expect(configuredBunkers(config).map(b => [b.id, b.origin, b.secret])).toEqual([
      ['main', 'http://bunker:3000', MAIN],
      ['root', 'http://bunker-root:3000', ROOT]
    ])
    expect(findBunker(config, 'root')?.label).toBe('Company root')
    expect(findBunker(config, 'elsewhere')).toBeUndefined()
  })

  it('refuses two bunkers sharing a secret, a repeated id or a bad id', () => {
    const shared = secretFile(MAIN)
    const list = (...entries: object[]) => ({ apiBase: 'http://x:3000', proxySecret: '', bunkers: JSON.stringify(entries) })
    expect(() => configuredBunkers(list(
      { id: 'main', origin: 'http://a:3000', secretFile: shared },
      { id: 'root', origin: 'http://b:3000', secretFile: shared }
    ))).toThrow('a proxy secret of its own')
    expect(() => configuredBunkers(list(
      { id: 'main', origin: 'http://a:3000', secretFile: secretFile(MAIN) },
      { id: 'main', origin: 'http://b:3000', secretFile: secretFile(ROOT) }
    ))).toThrow('twice')
    expect(() => configuredBunkers(list({ id: 'Root!', origin: 'http://a:3000', secretFile: shared }))).toThrow('lowercase')
  })
})
