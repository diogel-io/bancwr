import { describe, it, expect, beforeEach, vi } from 'vitest'
import { FakeRelays, nostrEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import { RELAY_LIST_INCOMPLETE_READ, RELAY_LIST_NO_RELAY_ON_SAVE, useRelayList } from '~/composables/useRelayList'
import { DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS } from '~/utils/profile-relays'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

const me = 'a'.repeat(64)
const EVERY_RELAY = [...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS]
const signer = (): NostrSigner & { signEvent: ReturnType<typeof vi.fn> } => ({
  signEvent: vi.fn(async (template: EventTemplate) => ({ ...template, id: 'f'.repeat(64), pubkey: me, sig: '' }) as SignedEvent)
})

describe('useRelayList', () => {
  let relays: FakeRelays

  beforeEach(() => {
    signInAs('signer')
    relays = new FakeRelays()
  })

  async function loaded() {
    const list = useRelayList({ io: relays })
    await list.load()
    return list
  }

  it('loads the newest list with its markers', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://old.example']], 100), nostrEvent(me, 10002, [['r', 'wss://a.example'], ['r', 'wss://b.example', 'read']], 200))
    const list = await loaded()
    expect(list.entries.value.map(e => [e.url, e.read, e.write])).toEqual([['wss://a.example', true, true], ['wss://b.example', true, false]])
  })

  it('stages adds, removes and markers, and unticking both markers removes the relay', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example'], ['r', 'wss://b.example']], 100))
    const list = await loaded()
    list.stageAdd('wss://c.example')
    list.stageMarkers('wss://a.example', { read: false, write: true })
    list.stageMarkers('wss://b.example/', { read: false, write: false })
    expect(list.removes.value).toEqual(['wss://b.example/'])
    expect(list.preview.value.map(e => [e.url, e.read, e.write])).toEqual([['wss://a.example', false, true], ['wss://c.example', true, true]])
    list.stageMarkers('wss://a.example', { read: true, write: true })
    expect(list.markers.value).toEqual({})
  })

  it('publishes onto the re-read list, keeping a relay added elsewhere, to old and new relays, defaults and indexers', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example'], ['r', 'wss://gone.example', 'write'], ['alt', 'x']], 100))
    const list = await loaded()
    list.stageRemove('wss://gone.example')
    list.stageAdd('wss://new.example')
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example'], ['r', 'wss://gone.example', 'write'], ['alt', 'x'], ['r', 'wss://elsewhere.example']], 150))

    const sign = signer()
    await list.save(sign)

    const published = relays.published[0]!
    expect(published.kind).toBe(10002)
    expect(published.tags).toEqual([['r', 'wss://a.example'], ['alt', 'x'], ['r', 'wss://elsewhere.example'], ['r', 'wss://new.example']])
    expect(published.created_at).toBeGreaterThan(150)
    expect(relays.publishedTo[0]).toEqual(expect.arrayContaining(['wss://gone.example/', 'wss://new.example/', 'wss://elsewhere.example/', ...EVERY_RELAY]))
    expect(list.dirty.value).toBe(false)
  })

  it('makes the profile and follows read through the new list afterwards', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example']], 100))
    const list = await loaded()
    list.stageAdd('wss://new.example')
    await list.save(signer())
    expect(list.relays.value[0]).toBe('wss://a.example/')
    expect(list.relays.value).toContain('wss://new.example/')
  })

  it('publishes nothing when no relay answers the re-read', async () => {
    relays.events.push(nostrEvent(me, 10002, [['r', 'wss://a.example']], 100))
    const list = await loaded()
    list.stageAdd('wss://b.example')
    relays.down = new Set([...EVERY_RELAY, 'wss://a.example/'])
    const sign = signer()
    await expect(list.save(sign)).rejects.toThrow(RELAY_LIST_NO_RELAY_ON_SAVE)
    expect(sign.signEvent).not.toHaveBeenCalled()
    expect(relays.published).toEqual([])
  })

  it('publishes nothing when the re-read finds an older list than the one loaded (incomplete read)', async () => {
    const older = nostrEvent(me, 10002, [['r', 'wss://a.example']], 100)
    const newer = nostrEvent(me, 10002, [['r', 'wss://a.example'], ['r', 'wss://b.example'], ['r', 'wss://c.example']], 200)
    relays.events.push(older, newer)
    relays.only = { [EVERY_RELAY[0]!]: [newer.id] }
    const list = await loaded()
    expect(list.entries.value).toHaveLength(3)

    list.stageAdd('wss://d.example')
    relays.down = new Set([EVERY_RELAY[0]!])
    const sign = signer()
    await expect(list.save(sign)).rejects.toThrow(RELAY_LIST_INCOMPLETE_READ)
    expect(sign.signEvent).not.toHaveBeenCalled()
    expect(relays.published).toEqual([])
  })

  it('fails to load when no relay answers, reports no list as not found, and records which relays answered', async () => {
    relays.down = new Set(EVERY_RELAY)
    expect((await loaded()).state.value).toBe('failed')

    relays.down = new Set([EVERY_RELAY[1]!])
    const none = await loaded()
    expect(none.exists.value).toBe(false)
    expect(none.unanswered.value.has(EVERY_RELAY[1]!)).toBe(true)
    expect(none.answered.value.has(EVERY_RELAY[0]!)).toBe(true)
  })
})
