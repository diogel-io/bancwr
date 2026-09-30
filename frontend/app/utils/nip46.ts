// Parsing a NIP-46 `bunker://` connection string (#11). Deliberately strict and local: nostr-tools'
// parseBunkerInput falls back to a NIP-05 lookup, fetching from whatever domain the input names,
// and the sign-in screen accepts only bunker:// URIs.

export interface BunkerPointer {
  /** The remote signer's pubkey, lowercase hex. */
  pubkey: string
  relays: string[]
  secret: string | null
}

/** The pointer, or a message saying what is wrong with the input. */
export function parseBunkerUri(input: string): BunkerPointer | { error: string } {
  const value = input.trim()
  if (!value.startsWith('bunker://')) {
    return { error: 'Enter a bunker:// connection string from your remote signer.' }
  }
  let url: URL
  try {
    url = new URL(value)
  } catch {
    return { error: 'This bunker:// connection string is not valid.' }
  }
  const pubkey = url.hostname.toLowerCase()
  if (!/^[0-9a-f]{64}$/.test(pubkey)) {
    return { error: 'The connection string must start with bunker:// followed by the signer\'s 64-character hex public key.' }
  }
  const relays = url.searchParams.getAll('relay').filter(relay => /^wss?:\/\//.test(relay))
  if (relays.length === 0) {
    return { error: 'The connection string names no relay (relay=wss://…).' }
  }
  return { pubkey, relays, secret: url.searchParams.get('secret') }
}
