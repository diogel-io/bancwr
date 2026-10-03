import { describe, it, expect, vi } from 'vitest'
import { lookupKeyProfile, lookupRelays } from '~/utils/key-profile'
import { DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS } from '~/utils/profile-relays'
import { profileEvent } from '../helpers/relays'

const key = 'a'.repeat(64)

describe('key profile lookup (#70)', () => {
  it('names the key from its newest profile', async () => {
    const query = vi.fn(async () => [profileEvent(key, { name: 'old' }, 1), profileEvent(key, { display_name: 'Alice' }, 2), profileEvent('b'.repeat(64), { name: 'someone else' }, 3)])
    expect(await lookupKeyProfile(key, ['wss://r.example/'], query)).toMatchObject({ name: 'Alice' })
    expect(query).toHaveBeenCalledWith(['wss://r.example/'], { kinds: [0], authors: [key] }, 3000)
  })

  it('gives up after the timeout, and on any failure or nothing found', async () => {
    expect(await lookupKeyProfile(key, [], () => new Promise(() => {}), 20)).toBeUndefined()
    expect(await lookupKeyProfile(key, [], async () => {
      throw new Error('relays down')
    })).toBeUndefined()
    expect(await lookupKeyProfile(key, [], async () => [])).toBeUndefined()
    expect(await lookupKeyProfile(key, [], async () => [{ ...profileEvent(key, {}, 1), content: 'not json' }])).toMatchObject({ name: undefined })
  })

  it('looks on the configured relays, or the defaults, and the indexers', () => {
    expect(lookupRelays('', '')).toEqual([...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS])
    expect(lookupRelays('wss://mine.example', 'wss://index.example')).toEqual(['wss://mine.example/', 'wss://index.example/'])
  })
})
