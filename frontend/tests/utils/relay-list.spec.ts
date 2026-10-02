import { describe, it, expect } from 'vitest'
import { applyRelayChanges, onlyStagedRelayChanges, parseRelayEntries, relayTag, type RelayChanges } from '~/utils/relay-list'

// A list as other clients leave it: both, read-only, write-only, a trailing slash, an unrelated tag.
const latest = {
  tags: [['r', 'wss://both.example/'], ['r', 'wss://read.example', 'read'], ['alt', 'relay list'], ['r', 'wss://write.example', 'write']],
  content: ''
}
const none: RelayChanges = { adds: [], removes: [], markers: {} }

describe('parseRelayEntries', () => {
  it('reads no marker as both, and keeps each tag as it was', () => {
    expect(parseRelayEntries(latest).map(e => [e.url, e.read, e.write])).toEqual([
      ['wss://both.example/', true, true], ['wss://read.example', true, false], ['wss://write.example', false, true]
    ])
    expect(parseRelayEntries({ tags: [['r', 'wss://x.example'], ['r', 'wss://X.example/'], ['r', 'https://no.example']] })).toHaveLength(1)
  })

  it('writes the shortest tag for the markers', () => {
    expect(relayTag('wss://a', { read: true, write: true })).toEqual(['r', 'wss://a'])
    expect(relayTag('wss://a', { read: true, write: false })).toEqual(['r', 'wss://a', 'read'])
    expect(relayTag('wss://a', { read: false, write: true })).toEqual(['r', 'wss://a', 'write'])
  })
})

describe('applyRelayChanges', () => {
  it('keeps untouched tags and content, rewrites only changed markers, drops removals and appends adds', () => {
    const changes: RelayChanges = {
      adds: [{ url: 'wss://new.example', read: true, write: true }],
      removes: ['wss://write.example/'],
      markers: { 'wss://read.example/': { read: true, write: true } }
    }
    const next = applyRelayChanges({ ...latest, content: 'kept' }, changes)
    expect(next.tags).toEqual([['r', 'wss://both.example/'], ['r', 'wss://read.example'], ['alt', 'relay list'], ['r', 'wss://new.example']])
    expect(next.content).toBe('kept')
    expect(onlyStagedRelayChanges({ ...latest, content: 'kept' }, next, changes)).toBe(true)
  })

  it('leaves the list exactly as it was with nothing staged', () => {
    expect(applyRelayChanges(latest, none)).toEqual(latest)
  })

  it('does not add a relay already listed under another spelling', () => {
    expect(applyRelayChanges(latest, { ...none, adds: [{ url: 'wss://both.example', read: true, write: true }] }).tags).toEqual(latest.tags)
  })

  it('fails the staged-changes check for anything else', () => {
    const next = applyRelayChanges(latest, none)
    expect(onlyStagedRelayChanges(latest, { ...next, tags: next.tags.filter(t => t[1] !== 'wss://read.example') }, none)).toBe(false)
    expect(onlyStagedRelayChanges(latest, { ...next, tags: next.tags.map(t => t[1] === 'wss://read.example' ? ['r', t[1]] : t) }, none)).toBe(false)
    expect(onlyStagedRelayChanges(latest, { ...next, tags: next.tags.filter(t => t[0] !== 'alt') }, none)).toBe(false)
    expect(onlyStagedRelayChanges(latest, { ...next, content: 'x' }, none)).toBe(false)
  })
})
