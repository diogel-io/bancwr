// @vitest-environment node
// The /api/bunker/* proxy route forwards the body exactly as it arrived and signs exactly those
// bytes: the bunker hashes what it receives, so a body parsed and re-serialised on the way would
// no longer match its signature (#25).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mockNuxtImport } from '@nuxt/test-utils/runtime'
import { PROXY_HEADERS, proxySignature } from '../../server/utils/auth/proxy-signature'

vi.mock('../../server/utils/auth/session', () => ({
  sessionPubkey: vi.fn(async () => 'ab'.repeat(32))
}))

const SECRET = 'bancwr-test-proxy-secret'
const PUBKEY = 'ab'.repeat(32)
const PATH = '/api/bunker/connections/tokens'

interface FakeRequest {
  method: string
  path: string
  rawBody: string | undefined
}

// The route reads its config through the auto-imported useRuntimeConfig.
mockNuxtImport('useRuntimeConfig', () => () => ({ apiBase: 'http://bunker:3000', proxySecret: 'bancwr-test-proxy-secret' }))

let request: FakeRequest
const $fetch = vi.fn(async (..._args: unknown[]) => ({ ok: true }))

beforeEach(() => {
  $fetch.mockClear()
  vi.stubGlobal('defineEventHandler', (handler: unknown) => handler)
  vi.stubGlobal('getRequestURL', () => new URL(`http://console.example${request.path}`))
  vi.stubGlobal('getMethod', () => request.method)
  vi.stubGlobal('readRawBody', async () => request.rawBody)
  vi.stubGlobal('createError', (input: { statusCode: number }) => Object.assign(new Error('error'), input))
  vi.stubGlobal('$fetch', $fetch)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

async function proxy(method: string, path: string, rawBody: string | undefined) {
  request = { method, path, rawBody }
  const { default: handler } = await import('../../server/api/bunker/[...]')
  await (handler as (event: unknown) => Promise<unknown>)({})
  expect($fetch).toHaveBeenCalledTimes(1)
  const [url, options] = $fetch.mock.calls[0] as [string, { method: string, body?: string, headers: Record<string, string> }]
  return { url, options }
}

/** The signature the bunker expects for a request with these headers and this body. */
function expected(headers: Record<string, string>, method: string, path: string, body: string) {
  return proxySignature(SECRET, {
    timestamp: Number(headers[PROXY_HEADERS.timestamp]),
    nonce: headers[PROXY_HEADERS.nonce]!,
    method,
    path,
    identity: headers[PROXY_HEADERS.identity]!,
    body
  })
}

describe('the bunker proxy route', () => {
  it('forwards the raw body unchanged and signs exactly it', async () => {
    // Spacing, key order and non-ASCII text that parsing and re-serialising would change.
    const raw = '{ "label" : "Café ☕",\n  "kinds":[1, 0] ,"label2":"x" }'
    const { url, options } = await proxy('POST', `${PATH}?x=1`, raw)

    expect(url).toBe(`http://bunker:3000${PATH}?x=1`)
    expect(options.method).toBe('POST')
    expect(options.body).toBe(raw)
    expect(options.headers[PROXY_HEADERS.identity]).toBe(PUBKEY)
    expect(options.headers[PROXY_HEADERS.nonce]).toMatch(/^[0-9a-f]{32}$/)
    expect(options.headers[PROXY_HEADERS.signature]).toBe(expected(options.headers, 'POST', `${PATH}?x=1`, raw))
    // Not a signature over the body as JSON would re-serialise it.
    const reserialised = JSON.stringify(JSON.parse(raw))
    expect(reserialised).not.toBe(raw)
    expect(options.headers[PROXY_HEADERS.signature]).not.toBe(expected(options.headers, 'POST', `${PATH}?x=1`, reserialised))
  })

  it('sends no body for an empty one and signs the empty body', async () => {
    const { options } = await proxy('POST', PATH, undefined)
    expect(options.body).toBeUndefined()
    expect(options.headers[PROXY_HEADERS.signature]).toBe(expected(options.headers, 'POST', PATH, ''))
  })

  it('signs a GET over an empty body without reading one', async () => {
    const { options } = await proxy('GET', '/api/bunker/team', 'ignored')
    expect(options.body).toBeUndefined()
    expect(options.headers[PROXY_HEADERS.signature]).toBe(expected(options.headers, 'GET', '/api/bunker/team', ''))
  })
})
