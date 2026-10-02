import { describe, it, expect, beforeEach, vi } from 'vitest'
import { npubEncode } from 'nostr-tools/nip19'
import { FakeRelays, nostrEvent, profileEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import { INCOMPLETE_READ, NO_RELAY_ON_SAVE, PROFILE_CHUNK, useFollows } from '~/composables/useFollows'
import { DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS } from '~/utils/profile-relays'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

const me = 'a'.repeat(64)
const key = (n: number) => n.toString(16).padStart(64, '0')
const EVERY_RELAY = [...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS]
const signer = (): NostrSigner & { signEvent: ReturnType<typeof vi.fn> } => ({
  signEvent: vi.fn(async (template: EventTemplate) => ({ ...template, id: 'f'.repeat(64), pubkey: me, sig: '' }) as SignedEvent)
})

describe('useFollows', () => {
  let relays: FakeRelays

  beforeEach(() => {
    signInAs('user')
    relays = new FakeRelays()
  })

  async function loaded() {
    const list = useFollows({ io: relays })
    await list.load()
    return list
  }

  it('loads the newest list across the relays', async () => {
    relays.events.push(nostrEvent(me, 3, [['p', key(1)]], 100), nostrEvent(me, 3, [['p', key(1)], ['p', key(2)]], 200), nostrEvent(key(9), 3, [['p', key(3)]], 300))
    const list = await loaded()
    expect(list.state.value).toBe('loaded')
    expect(list.follows.value.map(f => f.pubkey)).toEqual([key(1), key(2)])
  })

  it('stages adds and removes, collapsing npub and hex to one entry', async () => {
    relays.events.push(nostrEvent(me, 3, [['p', key(1)]], 100))
    const list = await loaded()
    list.stageAdd(key(2))
    list.stageAdd(key(2))
    list.stageRemove(key(1))
    expect(list.pendingAdds.value).toEqual([key(2)])
    expect(list.pendingRemoves.value).toEqual([key(1)])
    list.stageAdd(key(1)) // undo the removal
    list.stageRemove(key(2)) // undo the add
    expect(list.dirty.value).toBe(false)
    expect(npubEncode(key(2))).toMatch(/^npub1/)
  })

  it('publishes the staged changes onto the list as re-read, keeping other tags, content and follows made elsewhere', async () => {
    relays.events.push(nostrEvent(me, 3, [['p', key(1), 'wss://hint.example/', 'pet'], ['t', 'nostr'], ['p', key(2)]], 100, '{"relays":"kept"}'))
    const list = await loaded()
    list.stageAdd(key(3))
    list.stageRemove(key(2))
    // Another app follows key(4) after the page loaded.
    relays.events.push(nostrEvent(me, 3, [['p', key(1), 'wss://hint.example/', 'pet'], ['t', 'nostr'], ['p', key(2)], ['p', key(4)]], 150, '{"relays":"kept"}'))

    const sign = signer()
    await list.save(sign)

    const published = relays.published[0]!
    expect(published.kind).toBe(3)
    expect(published.tags).toEqual([['p', key(1), 'wss://hint.example/', 'pet'], ['t', 'nostr'], ['p', key(4)], ['p', key(3)]])
    expect(published.content).toBe('{"relays":"kept"}')
    expect(published.created_at).toBeGreaterThan(150)
    expect(list.dirty.value).toBe(false)
    expect(list.follows.value.map(f => f.pubkey)).toEqual([key(1), key(4), key(3)])
  })

  it('publishes nothing when no relay answers the re-read (no truncated list)', async () => {
    relays.events.push(nostrEvent(me, 3, [['p', key(1)], ['p', key(2)]], 100))
    const list = await loaded()
    list.stageRemove(key(1))
    relays.down = new Set(EVERY_RELAY)

    const sign = signer()
    await expect(list.save(sign)).rejects.toThrow(NO_RELAY_ON_SAVE)
    expect(sign.signEvent).not.toHaveBeenCalled()
    expect(relays.published).toEqual([])
  })

  it('publishes nothing when the re-read finds an older list than the one loaded (incomplete read)', async () => {
    const older = nostrEvent(me, 3, [['p', key(1)]], 100)
    const newer = nostrEvent(me, 3, [['p', key(1)], ['p', key(2)], ['p', key(3)]], 200)
    relays.events.push(older, newer)
    // Only the first relay holds the newer list; it answers the load, then not the save.
    relays.only = { [EVERY_RELAY[0]!]: [newer.id] }
    const list = await loaded()
    expect(list.follows.value).toHaveLength(3)

    list.stageAdd(key(4))
    relays.down = new Set([EVERY_RELAY[0]!])
    const sign = signer()
    await expect(list.save(sign)).rejects.toThrow(INCOMPLETE_READ)
    expect(sign.signEvent).not.toHaveBeenCalled()
    expect(relays.published).toEqual([])
  })

  it('fails to load when no relay answers, and reports a key with no list as not found', async () => {
    relays.down = new Set(EVERY_RELAY)
    const failed = await loaded()
    expect(failed.state.value).toBe('failed')

    relays.down = new Set()
    const none = await loaded()
    expect(none.state.value).toBe('loaded')
    expect(none.exists.value).toBe(false)
    expect(none.searched.value.reached).toEqual(EVERY_RELAY)
  })

  it('looks names up in chunks, newest profile per key', async () => {
    const keys = Array.from({ length: PROFILE_CHUNK + 10 }, (_, i) => key(i + 1))
    relays.events.push(nostrEvent(me, 3, keys.map(k => ['p', k]), 100))
    relays.events.push(profileEvent(key(1), { name: 'old' }, 1), profileEvent(key(1), { name: 'new' }, 2))
    const list = await loaded()
    await vi.waitFor(() => expect(list.profiles.value[key(1)]?.name).toBe('new'))
    const lookups = relays.filters.filter(f => f.kinds?.includes(0))
    expect(lookups).toHaveLength(2)
    expect(lookups[0]!.authors).toHaveLength(PROFILE_CHUNK)
  })
})
