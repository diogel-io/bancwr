import { describe, it, expect, vi } from 'vitest'
import { parseNip05Identifier, resolveNip05, verifyNip05 } from '~/utils/nip05'

const pubkey = 'ab'.repeat(32)
const answer = (body: unknown, status = 200) => vi.fn(async () => new Response(typeof body === 'string' ? body : JSON.stringify(body), { status }))

describe('parseNip05Identifier', () => {
  it('accepts name@domain and rejects anything else', () => {
    expect(parseNip05Identifier(' alice@example.com ')).toEqual({ name: 'alice', domain: 'example.com' })
    expect(parseNip05Identifier('_@example.com')).toEqual({ name: '_', domain: 'example.com' })
    for (const bad of ['', 'alice', 'alice@', '@example.com', 'a@b@c.com', 'alice@localhost', 'alice@example.com:8080', 'alice@example.com/path', 'al ice@example.com']) {
      expect(parseNip05Identifier(bad), bad).toBeNull()
    }
  })
})

describe('verifyNip05', () => {
  it('asks the domain\'s well-known document for the name, with no redirects', async () => {
    const fetcher = answer({ names: { alice: pubkey } })
    expect(await verifyNip05('alice@example.com', pubkey, fetcher)).toEqual({ status: 'verified', identifier: 'alice@example.com', domain: 'example.com' })
    const [url, init] = fetcher.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('https://example.com/.well-known/nostr.json?name=alice')
    expect(init.redirect).toBe('error')
  })

  it('reports each way it can fail', async () => {
    expect((await verifyNip05('alice', pubkey, answer({}))).status).toBe('malformed')
    expect((await verifyNip05('alice@example.com', pubkey, answer({ names: { alice: 'cd'.repeat(32) } }))).status).toBe('pubkey-mismatch')
    expect((await verifyNip05('alice@example.com', pubkey, answer({ names: { bob: pubkey } }))).status).toBe('not-found')
    expect((await verifyNip05('alice@example.com', pubkey, answer('', 404))).status).toBe('not-found')
    expect((await verifyNip05('alice@example.com', pubkey, answer('', 500))).status).toBe('network-error')
    expect((await verifyNip05('alice@example.com', pubkey, answer('not json'))).status).toBe('invalid-response')
    expect((await verifyNip05('alice@example.com', pubkey, answer({ names: { alice: 7 } }))).status).toBe('invalid-response')
    expect((await verifyNip05('alice@example.com', pubkey, vi.fn(async () => {
      throw new TypeError('Failed to fetch')
    }))).status).toBe('network-error')
  })

  it('never fetches for a malformed identifier', async () => {
    const fetcher = answer({})
    await verifyNip05('not an identifier', pubkey, fetcher)
    expect(fetcher).not.toHaveBeenCalled()
  })
})

describe('resolveNip05 (#32)', () => {
  it('returns the key the domain lists, lowercased', async () => {
    expect(await resolveNip05('alice@example.com', answer({ names: { alice: pubkey.toUpperCase() } }))).toEqual({ pubkey })
  })

  it('says why when it cannot', async () => {
    expect(await resolveNip05('alice', answer({}))).toEqual({ status: 'malformed' })
    expect(await resolveNip05('alice@example.com', answer({ names: {} }))).toEqual({ status: 'not-found' })
    expect(await resolveNip05('alice@example.com', answer({ names: { alice: 'not-hex' } }))).toEqual({ status: 'invalid-response' })
    expect(await resolveNip05('alice@example.com', answer('', 503))).toEqual({ status: 'network-error' })
  })
})
