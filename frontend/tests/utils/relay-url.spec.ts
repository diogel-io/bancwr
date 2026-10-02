import { describe, it, expect } from 'vitest'
import { isInsecureRemote, MAX_RELAY_URL_LENGTH, normalizeRelayUrl, relayKey, sameRelay } from '~/utils/relay-url'

describe('normalizeRelayUrl (Porwr\'s rules)', () => {
  it('trims, lowercases the host, and drops the trailing slash of an empty path', () => {
    expect(normalizeRelayUrl('  wss://Relay.Example.COM/ ')).toEqual({ url: 'wss://relay.example.com', hostname: 'relay.example.com' })
    expect(normalizeRelayUrl('ws://localhost:8080')).toEqual({ url: 'ws://localhost:8080', hostname: 'localhost' })
    expect(normalizeRelayUrl('wss://relay.example.com/path/')).toEqual({ url: 'wss://relay.example.com/path/', hostname: 'relay.example.com' })
  })

  it('refuses empty, malformed, non-websocket, fragment, bad hostname and over-long addresses', () => {
    for (const bad of ['', '   ', 'relay.example.com', 'https://relay.example.com', 'wss://relay.example.com/#x', 'wss://.example.com', `wss://${'a'.repeat(MAX_RELAY_URL_LENGTH)}.com`]) {
      expect(normalizeRelayUrl(bad), bad).toHaveProperty('error')
    }
  })
})

describe('relay identity', () => {
  it('treats a trailing slash and case as the same relay', () => {
    expect(sameRelay('wss://Relay.example.com', 'wss://relay.example.com/')).toBe(true)
    expect(sameRelay('wss://a.example', 'wss://b.example')).toBe(false)
    expect(relayKey('wss://a.example')).toBe('wss://a.example/')
  })

  it('flags unencrypted relays that are not local', () => {
    expect(isInsecureRemote('ws://relay.example.com')).toBe(true)
    for (const fine of ['wss://relay.example.com', 'ws://localhost:8080', 'ws://127.0.0.1:7777', 'ws://box.local']) expect(isInsecureRemote(fine), fine).toBe(false)
  })
})
