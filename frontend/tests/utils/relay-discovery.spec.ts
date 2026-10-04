import { describe, it, expect } from 'vitest'
import { FALLBACK_RELAYS, filterRelays, isBunkerRelayUrl, knownRelays, parseRelayDiscovery, RELAY_DISCOVERY_KIND } from '~/utils/relay-discovery'

const report = (tags: string[][], created_at = 100, kind = RELAY_DISCOVERY_KIND) => ({ kind, tags, created_at })

describe('NIP-66 relay discovery (#78)', () => {
  it('reads the relay, its open time and its auth and payment requirements', () => {
    expect(parseRelayDiscovery(report([
      ['d', 'wss://Some.Relay/'], ['n', 'clearnet'], ['R', '!payment'], ['R', 'auth'], ['rtt-open', '234'], ['N', '46']
    ]))).toEqual({ url: 'wss://some.relay', rttOpen: 234, auth: true, payment: false, seenAt: 100 })
  })

  it('leaves unknown what the monitor did not report', () => {
    expect(parseRelayDiscovery(report([['d', 'wss://quiet.example']]))).toEqual({ url: 'wss://quiet.example', rttOpen: undefined, auth: undefined, payment: undefined, seenAt: 100 })
    expect(parseRelayDiscovery(report([['d', 'wss://x.example'], ['rtt-open', 'fast']]))?.rttOpen).toBeUndefined()
    expect(parseRelayDiscovery(report([['d', 'wss://x.example'], ['rtt-open', '-5']]))?.rttOpen).toBeUndefined()
  })

  it('takes the last R tag for a requirement given twice', () => {
    expect(parseRelayDiscovery(report([['d', 'wss://x.example'], ['R', 'payment'], ['R', '!payment']]))?.payment).toBe(false)
  })

  it('is nothing for another kind, a missing d tag, or a d tag that is not a relay address', () => {
    expect(parseRelayDiscovery(report([['d', 'wss://x.example']], 100, 1))).toBeUndefined()
    expect(parseRelayDiscovery(report([['rtt-open', '10']]))).toBeUndefined()
    // NIP-66 allows a hex pubkey for a relay without a URL.
    expect(parseRelayDiscovery(report([['d', 'a'.repeat(64)]]))).toBeUndefined()
    expect(parseRelayDiscovery(report([['d', 'https://x.example']]))).toBeUndefined()
  })

  it('keeps one entry per relay, its newest report, fastest first and unknown times last', () => {
    const relays = knownRelays([
      report([['d', 'wss://slow.example'], ['rtt-open', '900']]),
      report([['d', 'wss://fast.example/'], ['rtt-open', '500']], 100),
      report([['d', 'wss://fast.example'], ['rtt-open', '50']], 200),
      report([['d', 'wss://fast.example'], ['rtt-open', '700']], 150),
      report([['d', 'wss://unknown.example']]),
      report([['d', 'not a relay']])
    ])
    expect(relays.map(r => [r.url, r.rttOpen])).toEqual([
      ['wss://fast.example', 50],
      ['wss://slow.example', 900],
      ['wss://unknown.example', undefined]
    ])
  })

  it('filters by address, ignoring case and the scheme', () => {
    const relays = [{ url: 'wss://relay.damus.io' }, { url: 'wss://nos.lol' }]
    expect(filterRelays(relays, 'DAMUS').map(r => r.url)).toEqual(['wss://relay.damus.io'])
    expect(filterRelays(relays, 'wss://nos').map(r => r.url)).toEqual(['wss://nos.lol'])
    expect(filterRelays(relays, '  ')).toHaveLength(2)
    expect(filterRelays(relays, 'nothing')).toEqual([])
  })

  it('accepts what the bunker accepts: wss://, or ws:// on this machine', () => {
    for (const ok of ['wss://relay.example', 'ws://localhost:7777', 'ws://127.0.0.1:7777', 'ws://[::1]:7777']) expect(isBunkerRelayUrl(ok), ok).toBe(true)
    for (const bad of ['ws://relay.example', 'https://relay.example', 'relay.example']) expect(isBunkerRelayUrl(bad), bad).toBe(false)
  })

  it('has a short fallback list of usable, normalised relays', () => {
    expect(FALLBACK_RELAYS.length).toBeGreaterThanOrEqual(3)
    expect(FALLBACK_RELAYS.length).toBeLessThanOrEqual(6)
    for (const relay of FALLBACK_RELAYS) {
      expect(isBunkerRelayUrl(relay.url), relay.url).toBe(true)
      expect(relay.url.endsWith('/'), relay.url).toBe(false)
    }
  })
})
