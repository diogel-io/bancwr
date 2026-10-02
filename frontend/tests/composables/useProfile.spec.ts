import { describe, it, expect, beforeEach, vi } from 'vitest'
import { FakeRelays, profileEvent } from '../helpers/relays'
import { signInAs } from '../helpers/session'
import { NO_RELAY_ON_SAVE, useProfile } from '~/composables/useProfile'
import { DEFAULT_INDEXER_RELAYS, DEFAULT_PROFILE_RELAYS } from '~/utils/profile-relays'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

const me = 'a'.repeat(64)
/** Signs as the session's key; the composable takes whatever signer the page hands it. */
const signer = (): NostrSigner & { signEvent: ReturnType<typeof vi.fn> } => ({
  signEvent: vi.fn(async (template: EventTemplate) => ({ ...template, id: 'f'.repeat(64), pubkey: me, sig: '' }) as SignedEvent)
})

describe('useProfile', () => {
  let relays: FakeRelays

  beforeEach(() => {
    signInAs('user')
    relays = new FakeRelays()
  })

  it('loads the newest profile across the relays into the form', async () => {
    relays.events.push(profileEvent(me, { name: 'old' }, 100), profileEvent(me, { name: 'new', about: 'hi' }, 200), profileEvent('b'.repeat(64), { name: 'someone else' }, 300))
    const profile = useProfile({ io: relays })
    await profile.load()

    expect(profile.state.value).toBe('loaded')
    expect(profile.exists.value).toBe(true)
    expect(profile.form.value.name).toBe('new')
    expect(profile.form.value.about).toBe('hi')
    expect(profile.dirty.value).toBe(false)
  })

  it('treats a key with no profile as a new one', async () => {
    const profile = useProfile({ io: relays })
    await profile.load()
    expect(profile.state.value).toBe('loaded')
    expect(profile.exists.value).toBe(false)
  })

  it('fails to load when no relay answers, rather than showing an empty form', async () => {
    const profile = useProfile({ io: relays })
    relays.down = new Set([...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS])
    await profile.load()
    expect(profile.state.value).toBe('failed')
  })

  it('saves every field and keeps the ones the form does not edit (#30)', async () => {
    relays.events.push(profileEvent(me, { name: 'alice', pronouns: 'they/them', lud06: 'lnurl1x', nip05: 'old@a.example' }, 100))
    const profile = useProfile({ io: relays })
    await profile.load()
    // Another client changes the profile after the page loaded it.
    relays.events.push(profileEvent(me, { name: 'alice', pronouns: 'they/them', lud06: 'lnurl1x', nip05: 'old@a.example', added_elsewhere: true }, 150))

    profile.form.value.display_name = 'Alice'
    profile.form.value.nip05 = ''
    const sign = signer()
    const result = await profile.save(sign)

    const content = JSON.parse(relays.published[0]!.content)
    expect(content).toEqual({ name: 'alice', display_name: 'Alice', pronouns: 'they/them', lud06: 'lnurl1x', added_elsewhere: true })
    const template = sign.signEvent.mock.calls[0]![0] as EventTemplate
    expect(template).toMatchObject({ kind: 0, tags: [] })
    expect(template.created_at).toBeGreaterThan(150)
    expect(result.accepted.length).toBeGreaterThan(0)
    expect(profile.dirty.value).toBe(false)
  })

  it('publishes after the newest event even when the clock is behind it', async () => {
    const future = Math.floor(Date.now() / 1000) + 3600
    relays.events.push(profileEvent(me, { name: 'a' }, future))
    const profile = useProfile({ io: relays })
    await profile.load()
    profile.form.value.name = 'b'
    const sign = signer()
    await profile.save(sign)
    expect((sign.signEvent.mock.calls[0]![0] as EventTemplate).created_at).toBe(future + 1)
  })

  it('refuses to save when no relay answers the re-read, and signs nothing', async () => {
    relays.events.push(profileEvent(me, { name: 'alice', other: 1 }, 100))
    const profile = useProfile({ io: relays })
    await profile.load()
    profile.form.value.name = 'changed'
    relays.down = new Set(profile.relays.value)

    const sign = signer()
    await expect(profile.save(sign)).rejects.toThrow(NO_RELAY_ON_SAVE)
    expect(sign.signEvent).not.toHaveBeenCalled()
    expect(relays.published).toEqual([])
    expect(profile.dirty.value).toBe(true)
  })

  it('reports a save no relay accepted, and keeps the changes unsaved', async () => {
    const profile = useProfile({ io: relays })
    await profile.load()
    profile.form.value.name = 'alice'
    relays.refuse = true
    await expect(profile.save(signer())).rejects.toThrow('No relay accepted')
    expect(profile.dirty.value).toBe(true)
  })

  it('uses the member\'s NIP-65 write relays and Blossom server when they have them', async () => {
    relays.events.push(
      { ...profileEvent(me, {}, 1), kind: 10002, content: '', tags: [['r', 'wss://mine.example'], ['r', 'wss://read.example', 'read']] },
      { ...profileEvent(me, {}, 1), kind: 10063, content: '', tags: [['server', 'https://cdn.example']] }
    )
    const profile = useProfile({ io: relays })
    await profile.load()
    expect(profile.relays.value[0]).toBe('wss://mine.example/')
    expect(profile.relays.value).not.toContain('wss://read.example/')
    expect(profile.blossomServer.value).toBe('https://cdn.example')
  })

  it('finds a profile and relay list held only by an indexer, whose own relays are down (#62)', async () => {
    // The reported case: the key's NIP-65 list names a relay that is down, and neither the list
    // nor the profile is on the default relays, only on an indexer.
    const indexer = DEFAULT_INDEXER_RELAYS[0]!
    const list = { ...profileEvent(me, {}, 50), kind: 10002, content: '', tags: [['r', 'wss://dead.example/'], ['r', DEFAULT_PROFILE_RELAYS[0]!]] }
    const kind0 = profileEvent(me, { name: 'gary_woodfine', display_name: 'Gary Woodfine', extra: 'kept' }, 100)
    relays.events.push(list, kind0)
    relays.only = { [indexer]: [list.id, kind0.id] }
    relays.down = new Set(['wss://dead.example/'])

    const profile = useProfile({ io: relays })
    await profile.load()

    expect(profile.state.value).toBe('loaded')
    expect(profile.exists.value).toBe(true)
    expect(profile.form.value.display_name).toBe('Gary Woodfine')
    expect(profile.relays.value).toContain('wss://dead.example/')
    expect(profile.searched.value.failed).toEqual(['wss://dead.example/'])

    profile.form.value.about = 'Hello'
    await profile.save(signer())
    expect(JSON.parse(relays.published[0]!.content)).toEqual({ name: 'gary_woodfine', display_name: 'Gary Woodfine', extra: 'kept', about: 'Hello' })
    // Published to the indexers as well, which also repairs the default relays.
    expect(relays.publishedTo[0]).toEqual(expect.arrayContaining([...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS]))
  })

  it('reports every relay it asked when no profile is found, and can search one more', async () => {
    const elsewhere = profileEvent(me, { name: 'elsewhere' }, 100)
    relays.events.push(elsewhere)
    relays.only = { 'wss://relay.elsewhere.example/': [elsewhere.id] }
    const profile = useProfile({ io: relays })
    await profile.load()

    expect(profile.exists.value).toBe(false)
    expect(profile.searched.value.reached).toEqual([...DEFAULT_PROFILE_RELAYS, ...DEFAULT_INDEXER_RELAYS])

    expect(await profile.searchRelay('https://relay.elsewhere.example')).toContain('wss://')
    expect(await profile.searchRelay('wss://relay.elsewhere.example')).toBeUndefined()
    expect(profile.exists.value).toBe(true)
    expect(profile.form.value.name).toBe('elsewhere')
  })
})
