import { describe, it, expect, beforeEach, vi } from 'vitest'
import { useState } from '#imports'
import { signInAs } from '../helpers/session'
import type { AuthState } from '~/composables/useAuth'
import { useSignedInProfile } from '~/composables/useSignedInProfile'

// The lookup is driven by hand, so a test can hold it open and answer it late.
const { lookups } = vi.hoisted(() => ({ lookups: [] as { pubkey: string, resolve: (p: unknown) => void }[] }))
vi.mock('~/utils/key-profile', () => ({
  lookupKeyProfile: (pubkey: string) => new Promise((resolve) => { lookups.push({ pubkey, resolve }) }),
  lookupRelays: () => [],
  queryRelays: async () => []
}))

const signInAsKey = (pubkey: string) => {
  useState<AuthState>('auth').value = { status: 'signed-in', pubkey, npub: `npub1${pubkey.slice(0, 4)}`, role: 'signer' }
}

describe('useSignedInProfile (#72)', () => {
  beforeEach(() => {
    lookups.length = 0
    useState('signed-in-profile').value = { pubkey: '', status: 'idle' }
    signInAs('signer')
  })

  it('looks the signed-in key up once, however many places ask', async () => {
    const footer = useSignedInProfile()
    const page = useSignedInProfile()
    const first = footer.load()
    void page.load()
    expect(footer.loading.value).toBe(true)
    expect(lookups).toHaveLength(1)

    lookups[0]!.resolve({ name: 'Alice', picture: 'https://img.example/a.png', nip05: 'x', createdAt: 1 })
    await first
    expect(page.profile.value).toEqual({ name: 'Alice', picture: 'https://img.example/a.png' })

    await footer.load()
    expect(lookups).toHaveLength(1)
  })

  it('looks again for a new key, and shows nothing of the previous one', async () => {
    const shared = useSignedInProfile()
    const first = shared.load()
    lookups[0]!.resolve({ name: 'Alice', createdAt: 1 })
    await first

    signInAsKey('b'.repeat(64))
    expect(shared.profile.value).toBeUndefined()
    void shared.load()
    expect(lookups.map(l => l.pubkey)).toEqual(['a'.repeat(64), 'b'.repeat(64)])
  })

  it('ignores a lookup for a previous key that answers late', async () => {
    const shared = useSignedInProfile()
    const stale = shared.load()
    signInAsKey('b'.repeat(64))
    const fresh = shared.load()

    lookups[1]!.resolve({ name: 'Bob', createdAt: 2 })
    await fresh
    lookups[0]!.resolve({ name: 'Alice', createdAt: 1 })
    await stale
    expect(shared.profile.value?.name).toBe('Bob')
  })

  it('takes what the profile page saved, over a lookup still in flight', async () => {
    const shared = useSignedInProfile()
    const pending = shared.load()
    shared.setFromContent(JSON.stringify({ display_name: 'Saved', name: 'saved', picture: 'https://img.example/new.png' }))
    lookups[0]!.resolve({ name: 'Old', picture: 'https://img.example/old.png', createdAt: 1 })
    await pending

    expect(shared.profile.value).toEqual({ name: 'Saved', picture: 'https://img.example/new.png' })
    expect(shared.loading.value).toBe(false)
  })

  it('keeps only an http(s) picture from saved content', () => {
    const shared = useSignedInProfile()
    shared.setFromContent(JSON.stringify({ name: 'x', picture: 'javascript:alert(1)' }))
    expect(shared.profile.value).toEqual({ name: 'x', picture: undefined })
  })

  it('does nothing when signed out', async () => {
    signInAs(undefined)
    const shared = useSignedInProfile()
    await shared.load()
    shared.setFromContent('{"name":"x"}')
    expect(lookups).toHaveLength(0)
    expect(shared.profile.value).toBeUndefined()
  })
})
