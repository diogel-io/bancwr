// The signer for one of the bunker's identities (#150): it sends only the template to the
// identity's sign route, and uses what comes back only if the identity signed it and it is valid.
import { describe, it, expect, vi } from 'vitest'
import { finalizeEvent, generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { SignerMismatch } from '~/composables/useUserSigner'
import { identitySigner, IdentitySignError } from '~/utils/identity-signer'
import type { EventTemplate, SignedEvent } from '~/utils/nostr-sign-in'

const secret = generateSecretKey()
const identity = getPublicKey(secret)
const template: EventTemplate = { kind: 0, created_at: 1_700_000_000, tags: [], content: '{"name":"Sales"}' }

describe('identitySigner', () => {
  it('sends only the template to the identity\'s sign route and returns the checked event', async () => {
    const post = vi.fn(async (_url: string, options: { body: EventTemplate }) => finalizeEvent({ ...options.body }, secret) as SignedEvent)
    const signed = await identitySigner(identity, post).signEvent({ ...template, pubkey: 'x', id: 'y' } as EventTemplate)

    expect(post).toHaveBeenCalledWith(`/api/bunker/identities/${identity}/sign`, { method: 'POST', body: template })
    expect(signed.pubkey).toBe(identity)
    expect(signed.content).toBe(template.content)
  })

  it('refuses an event signed by another key, or tampered with', async () => {
    const other = generateSecretKey()
    const wrongKey = vi.fn(async () => finalizeEvent({ ...template }, other) as SignedEvent)
    await expect(identitySigner(identity, wrongKey).signEvent(template)).rejects.toBeInstanceOf(SignerMismatch)

    const tampered = vi.fn(async () => ({ ...finalizeEvent({ ...template }, secret), content: '{"name":"Evil"}' }) as SignedEvent)
    await expect(identitySigner(identity, tampered).signEvent(template)).rejects.toBeInstanceOf(SignerMismatch)
  })

  it('gives the bunker\'s reason when it refuses', async () => {
    const refused = vi.fn(async () => {
      throw Object.assign(new Error('Bad Request'), { data: { data: { error: 'kind_not_allowed', message: 'The console can only sign a profile.' } } })
    })
    const failure = await identitySigner(identity, refused).signEvent(template).catch(e => e)
    expect(failure).toBeInstanceOf(IdentitySignError)
    expect(failure.message).toBe('The console can only sign a profile.')
  })
})
