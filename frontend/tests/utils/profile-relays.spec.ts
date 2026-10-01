import { describe, it, expect } from 'vitest'
import { blossomServerFrom, MAX_OWN_RELAYS, parseRelayList, profileRelays, writeRelays } from '~/utils/profile-relays'

describe('profile relays', () => {
  it('parses the configured list, normalising and dropping what is not a relay', () => {
    expect(parseRelayList(' wss://relay.damus.io , https://not.a.relay,wss://NOS.lol/ ,')).toEqual(['wss://relay.damus.io/', 'wss://nos.lol/'])
  })

  it('uses the NIP-65 relays marked write or not marked, never read-only ones', () => {
    const list = { tags: [['r', 'wss://both.example'], ['r', 'wss://write.example', 'write'], ['r', 'wss://read.example', 'read'], ['p', 'x']] }
    expect(writeRelays(list)).toEqual(['wss://both.example/', 'wss://write.example/'])
  })

  it('puts the member\'s relays first, then the defaults, once each', () => {
    const list = { tags: [['r', 'wss://mine.example'], ['r', 'wss://nos.lol']] }
    expect(profileRelays(['wss://nos.lol/', 'wss://relay.damus.io/'], list)).toEqual(['wss://mine.example/', 'wss://nos.lol/', 'wss://relay.damus.io/'])
    expect(profileRelays(['wss://nos.lol/'], undefined)).toEqual(['wss://nos.lol/'])
  })

  it('bounds how many of the member\'s relays are used', () => {
    const list = { tags: Array.from({ length: 20 }, (_, i) => ['r', `wss://r${i}.example`]) }
    expect(writeRelays(list)).toHaveLength(MAX_OWN_RELAYS)
  })

  it('takes the first https server of a Blossom list (BUD-03)', () => {
    expect(blossomServerFrom({ tags: [['server', 'http://insecure.example'], ['server', 'https://cdn.example/']] })).toBe('https://cdn.example')
    expect(blossomServerFrom(undefined)).toBeUndefined()
  })
})
