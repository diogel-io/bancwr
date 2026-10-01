import { describe, it, expect, vi } from 'vitest'
import { finalizeEvent, generateSecretKey } from 'nostr-tools/pure'
import { authorizationHeader, sha256Hex, uploadAuthTemplate, uploadToBlossom } from '~/utils/blossom'
import type { EventTemplate, NostrSigner, SignedEvent } from '~/utils/nostr-sign-in'

const key = generateSecretKey()
const signer: NostrSigner = { signEvent: async (t: EventTemplate) => finalizeEvent(t, key) as unknown as SignedEvent }
const blob = new Blob([new Uint8Array([1, 2, 3])], { type: 'image/webp' })

async function hash() {
  return sha256Hex(await blob.arrayBuffer())
}

describe('Blossom upload', () => {
  it('authorises exactly one blob, for five minutes (BUD-02)', () => {
    expect(uploadAuthTemplate('ff', 1000)).toEqual({
      kind: 24242, created_at: 1000, content: 'Upload profile image',
      tags: [['t', 'upload'], ['x', 'ff'], ['expiration', '1300']]
    })
  })

  it('sends the signed authorisation as base64 JSON', async () => {
    const event = await signer.signEvent(uploadAuthTemplate('ff'))
    const header = authorizationHeader(event)
    expect(header.startsWith('Nostr ')).toBe(true)
    // As JSON: the signed object also carries nostr-tools' verification cache, which is not sent.
    expect(JSON.parse(atob(header.slice(6)))).toEqual(JSON.parse(JSON.stringify(event)))
  })

  it('PUTs the blob and returns the URL the server gives for its hash', async () => {
    const sha256 = await hash()
    const fetcher = vi.fn(async () => Response.json({ url: `https://cdn.example/${sha256}.webp`, sha256 }))
    expect(await uploadToBlossom('https://cdn.example/', blob, signer, fetcher)).toBe(`https://cdn.example/${sha256}.webp`)
    const [url, init] = fetcher.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('https://cdn.example/upload')
    expect(init.method).toBe('PUT')
    const auth = JSON.parse(atob(String((init.headers as Record<string, string>).Authorization).slice(6)))
    expect(auth.tags).toContainEqual(['x', sha256])
  })

  it('refuses an answer for another hash, or one that is not https', async () => {
    await expect(uploadToBlossom('https://cdn.example', blob, signer, vi.fn(async () => Response.json({ url: 'https://cdn.example/x', sha256: '00' }))))
      .rejects.toThrow('did not match')
    const sha256 = await hash()
    await expect(uploadToBlossom('https://cdn.example', blob, signer, vi.fn(async () => Response.json({ url: 'http://cdn.example/x', sha256 }))))
      .rejects.toThrow('not https')
  })

  it('says why the server refused, and when the signer declined', async () => {
    await expect(uploadToBlossom('https://cdn.example', blob, signer, vi.fn(async () => new Response('', { status: 413, headers: { 'X-Reason': 'File too large' } }))))
      .rejects.toThrow('cdn.example refused the upload: File too large.')
    const declining: NostrSigner = { signEvent: async () => {
      throw new Error('no')
    } }
    const fetcher = vi.fn()
    await expect(uploadToBlossom('https://cdn.example', blob, declining, fetcher)).rejects.toThrow('did not sign the upload')
    expect(fetcher).not.toHaveBeenCalled()
  })
})
