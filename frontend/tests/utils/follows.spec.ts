import { describe, it, expect } from 'vitest'
import { npubEncode } from 'nostr-tools/nip19'
import { applyChanges, chunks, displayName, normalizePubkey, onlyStagedChanges, parseFollowProfile, parseFollows, validateFollowInput } from '~/utils/follows'

const a = 'a'.repeat(64)
const b = 'b'.repeat(64)
const c = 'c'.repeat(64)
const d = 'd'.repeat(64)

// A list as other clients leave it: hints, petnames, hashtags, an `a` tag, and a relay map in
// content (NIP-02 does not use it, but some clients still write it).
const latest = {
  tags: [['p', a, 'wss://alice.example/', 'alice'], ['t', 'nostr'], ['p', b], ['a', '30000:x:y'], ['p', c, '', 'carol']],
  content: '{"wss://relay.example/":{"read":true,"write":true}}'
}

describe('normalizePubkey', () => {
  it('gives the same key for hex in any case and for its npub', () => {
    expect(normalizePubkey(a.toUpperCase())).toBe(a)
    expect(normalizePubkey(` ${npubEncode(a)} `)).toBe(a)
  })

  it('refuses anything else', () => {
    for (const bad of ['', 'npub1nope', 'a'.repeat(63), 'note1qqqqqq', `${a}0`]) expect(normalizePubkey(bad), bad).toBeNull()
  })
})

describe('parseFollows', () => {
  it('takes each p tag once, keeping it as it was', () => {
    const follows = parseFollows({ tags: [...latest.tags, ['p', a.toUpperCase()], ['p', 'not-a-key']] })
    expect(follows.map(f => f.pubkey)).toEqual([a, b, c])
    expect(follows[0]!.tag).toEqual(['p', a, 'wss://alice.example/', 'alice'])
  })
})

describe('applyChanges', () => {
  it('keeps every tag not removed, in order, keeps content, and appends adds', () => {
    const next = applyChanges(latest, [d], [b])
    expect(next.tags).toEqual([['p', a, 'wss://alice.example/', 'alice'], ['t', 'nostr'], ['a', '30000:x:y'], ['p', c, '', 'carol'], ['p', d]])
    expect(next.content).toBe(latest.content)
  })

  it('does not duplicate a key already followed, and starts a first list empty', () => {
    expect(applyChanges(latest, [a], []).tags).toEqual(latest.tags)
    expect(applyChanges(undefined, [a], [])).toEqual({ tags: [['p', a]], content: '' })
  })

  it('passes the staged-changes check, and anything else fails it', () => {
    const next = applyChanges(latest, [d], [b])
    expect(onlyStagedChanges(latest, next, [d], [b])).toBe(true)
    // A follow dropped that was not removed.
    expect(onlyStagedChanges(latest, { ...next, tags: next.tags.filter(t => t[1] !== c) }, [d], [b])).toBe(false)
    // Another tag changed, or the content.
    expect(onlyStagedChanges(latest, { ...next, tags: next.tags.filter(t => t[0] !== 't') }, [d], [b])).toBe(false)
    expect(onlyStagedChanges(latest, { ...next, content: '' }, [d], [b])).toBe(false)
  })
})

describe('validateFollowInput', () => {
  it('accepts a new key by npub or hex, and refuses malformed, duplicate and own keys', () => {
    const following = new Set([a])
    expect(validateFollowInput(npubEncode(b), following, c)).toEqual({ pubkey: b })
    expect(validateFollowInput(b.toUpperCase(), following, c)).toEqual({ pubkey: b })
    expect(validateFollowInput('bob', following, c)).toHaveProperty('error')
    expect(validateFollowInput(npubEncode(a), following, c)).toEqual({ error: 'You already follow this key.' })
    expect(validateFollowInput(c, following, c)).toEqual({ error: 'That is your own key.' })
  })
})

describe('names', () => {
  it('reads name, picture and NIP-05 defensively', () => {
    expect(parseFollowProfile({ content: '{"display_name":"Alice","name":"alice","picture":"https://p.example/a.png","nip05":"a@a.example"}', created_at: 5 }))
      .toEqual({ name: 'Alice', picture: 'https://p.example/a.png', nip05: 'a@a.example', createdAt: 5 })
    expect(parseFollowProfile({ content: '{"name":"bob","picture":"javascript:x"}', created_at: 1 })).toEqual({ name: 'bob', picture: undefined, nip05: undefined, createdAt: 1 })
    expect(parseFollowProfile({ content: 'nonsense', created_at: 1 }).name).toBeUndefined()
  })

  it('shows the profile name, then the petname, then a short npub', () => {
    const follow = { pubkey: a, tag: ['p', a, '', 'pet'] }
    expect(displayName(follow, { name: 'Alice', createdAt: 1 })).toBe('Alice')
    expect(displayName(follow)).toBe('pet')
    expect(displayName({ pubkey: a, tag: ['p', a] })).toMatch(/^npub1.{7}….{6}$/)
  })

  it('chunks keys for lookups', () => {
    expect(chunks([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]])
  })
})
