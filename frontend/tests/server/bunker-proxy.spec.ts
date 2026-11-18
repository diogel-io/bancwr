// @vitest-environment node
// The proxy to the bunker, routed as Nitro routes it: h3's router matches the decoded path, so
// /api/%62unker/team reaches this handler. The rule that a body is forwarded only as JSON is the
// handler's own, so it holds for every path that reaches it.
import { describe, it, expect, beforeAll, beforeEach, vi } from 'vitest'
import * as h3 from 'h3'

vi.mock('../../server/utils/auth/session', () => ({ sessionPubkey: async () => 'a'.repeat(64) }))
// The test environment auto-imports useRuntimeConfig from the Nuxt app, not from Nitro.
vi.mock('#app/nuxt', async importOriginal => ({
  ...await importOriginal<object>(),
  useRuntimeConfig: () => ({ apiBase: 'http://bunker:3000', proxySecret: 'secret' })
}))
vi.mock('../../server/utils/auth/proxy-signature', () => ({ proxyHeaders: () => ({ 'X-Bancwr-Identity': 'a'.repeat(64) }) }))

const forwarded = vi.fn(async (_url: string, _init: { method: string, body?: unknown, headers: Record<string, string> }) => ({ ok: true }))

let handle: (request: Request) => Promise<Response>

beforeAll(async () => {
  for (const name of ['defineEventHandler', 'getMethod', 'getHeader', 'getRequestURL', 'createError', 'readRawBody', 'setResponseHeaders', 'sendStream'] as const) {
    vi.stubGlobal(name, h3[name])
  }
  vi.stubGlobal('$fetch', forwarded)

  const { default: proxy } = await import('../../server/api/bunker/[...]')
  const app = h3.createApp()
  const router = h3.createRouter().use('/api/bunker/**', proxy)
  app.use(router.handler)
  handle = h3.toWebHandler(app)
})

beforeEach(() => forwarded.mockClear())

/** POSTs as Node's HTTP server would hand it to Nitro: a body comes with its Content-Length. */
function post(path: string, init: { contentType?: string, body?: string } = {}) {
  // Bytes, not a string, so the Request adds no content type of its own.
  const body = init.body === undefined ? undefined : new TextEncoder().encode(init.body)
  return handle(new Request(`http://bancwr.example${path}`, {
    method: 'POST',
    headers: {
      ...(init.contentType ? { 'Content-Type': init.contentType } : {}),
      ...(body ? { 'Content-Length': String(body.byteLength) } : {})
    },
    body
  }))
}

describe('the proxy to the bunker', () => {
  it('forwards a JSON body with the type it was declared with', async () => {
    const response = await post('/api/bunker/team', { contentType: 'application/json; charset=utf-8', body: '{"name":"a"}' })

    expect(response.status).toBe(200)
    expect(forwarded).toHaveBeenCalledOnce()
    const [url, init] = forwarded.mock.calls[0]!
    expect(url).toBe('http://bunker:3000/api/bunker/team')
    // As the exact string that arrived: the signature covers it (GHSA-cmg4-5gjv-6rrr).
    expect(init.body).toBe('{"name":"a"}')
    expect(init.headers['Content-Type']).toBe('application/json; charset=utf-8')
  })

  it('forwards a request with no body without a content type', async () => {
    const response = await post('/api/bunker/approvals/1/reject')

    expect(response.status).toBe(200)
    const [, init] = forwarded.mock.calls[0]!
    expect(init.body).toBeUndefined()
    expect(init.headers['Content-Type']).toBeUndefined()
  })

  for (const path of ['/api/bunker/team', '/api/%62unker/team', '/api/bunker/%74eam']) {
    for (const contentType of [undefined, 'text/plain', 'application/x-www-form-urlencoded', 'multipart/form-data; boundary=x']) {
      it(`refuses a ${contentType ?? 'untyped'} body for ${path}, and forwards nothing`, async () => {
        const response = await post(path, { contentType, body: '{"name":"a"}' })

        expect(response.status).toBe(415)
        expect(await response.json()).toMatchObject({ data: { error: 'unsupported_media_type' } })
        expect(forwarded).not.toHaveBeenCalled()
      })
    }
  }

  it('refuses a chunked body that is not JSON', async () => {
    const body = new ReadableStream({ start(controller) { controller.enqueue(new TextEncoder().encode('a=b')); controller.close() } })
    const response = await handle(new Request('http://bancwr.example/api/%62unker/team', {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain', 'Transfer-Encoding': 'chunked' },
      body,
      // @ts-expect-error -- Node's fetch needs this for a stream body; the DOM types lack it.
      duplex: 'half'
    }))

    expect(response.status).toBe(415)
    expect(forwarded).not.toHaveBeenCalled()
  })
})
