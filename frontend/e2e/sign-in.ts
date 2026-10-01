// Signing in without a browser, for global setup and the signed-in fixture (#11): the same
// NIP-98 request the sign-in page makes, signed with a throwaway key held by the test.
import { createHash } from 'node:crypto'
import { finalizeEvent } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'

export interface PostLike {
  (url: string, init: { headers?: Record<string, string>, body?: string }): Promise<{ status: number, json: () => Promise<unknown> }>
}

/** Signs in as `nsec`, POSTing through `post` (which must keep the session cookie), and returns the login response. */
export async function signInAs(nsec: string, origin: string, post: PostLike) {
  const key = decode(nsec).data as Uint8Array
  const { challenge } = await (await post(`${origin}/api/auth/challenge`, {})).json() as { challenge: string }
  const body = JSON.stringify({ challenge })
  const event = finalizeEvent({
    kind: 27235,
    created_at: Math.floor(Date.now() / 1000),
    content: '',
    tags: [
      ['u', `${origin}/api/auth/login`],
      ['method', 'POST'],
      ['payload', createHash('sha256').update(body).digest('hex')]
    ]
  }, key)
  const response = await post(`${origin}/api/auth/login`, {
    headers: { 'Content-Type': 'application/json', 'Authorization': `Nostr ${Buffer.from(JSON.stringify(event)).toString('base64')}` },
    body
  })
  return { status: response.status, body: await response.json() }
}

/** A `post` over plain fetch that remembers the session cookie, for use outside a browser. */
export function cookieJarPost(): { post: PostLike, cookie: () => string } {
  let cookie = ''
  const post: PostLike = async (url, init) => {
    const response = await fetch(url, { method: 'POST', headers: { ...init.headers, ...(cookie ? { cookie } : {}) }, body: init.body })
    const set = response.headers.get('set-cookie')
    if (set) cookie = set.split(';')[0]!
    return response
  }
  return { post, cookie: () => cookie }
}
