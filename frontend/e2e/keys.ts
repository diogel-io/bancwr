// Throwaway Nostr keys for the suite. Nothing here is ever a real key: each run's bunker key is
// generated fresh and exists only in the environment of this process and the bunker container.
import { generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { decode, npubEncode, nsecEncode } from 'nostr-tools/nip19'

export function generateNsec(): string {
  return nsecEncode(generateSecretKey())
}

export function npubFromNsec(nsec: string): string {
  const decoded = decode(nsec)
  if (decoded.type !== 'nsec') {
    throw new Error('BUNKER_NSEC is not an nsec')
  }
  return npubEncode(getPublicKey(decoded.data))
}

/** A valid npub nobody holds the key to, for team members. */
export function randomNpub(): string {
  return npubEncode(getPublicKey(generateSecretKey()))
}
