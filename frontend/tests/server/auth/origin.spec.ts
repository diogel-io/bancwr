import { describe, it, expect } from 'vitest'
import { refusal, type IncomingRequest } from '../../../server/utils/auth/origin'

const SITE = 'https://bancwr.example'

function request(overrides: Partial<IncomingRequest>): IncomingRequest {
  return { method: 'POST', path: '/api/bunker/team', hasBody: false, ...overrides }
}

describe('which requests the console accepts', () => {
  it('lets the console\'s own requests through', () => {
    expect(refusal(request({ origin: SITE, contentType: 'application/json', hasBody: true }), SITE)).toBeUndefined()
    expect(refusal(request({ method: 'DELETE', path: '/api/bunker/team/1', origin: SITE }), SITE)).toBeUndefined()
    expect(refusal(request({ path: '/api/auth/logout', origin: SITE }), SITE)).toBeUndefined()
    expect(refusal(request({ origin: SITE, contentType: 'application/json; charset=utf-8', hasBody: true }), SITE)).toBeUndefined()
  })

  it('refuses a request that changes something from another origin, on the same site or not', () => {
    for (const origin of ['https://evil.example', 'https://other.bancwr.example', 'http://bancwr.example', 'null']) {
      for (const method of ['POST', 'PUT', 'PATCH', 'DELETE']) {
        expect(refusal(request({ method, origin, contentType: 'application/json', hasBody: true }), SITE)?.data)
          .toEqual({ error: 'forbidden', reason: 'bad_origin' })
      }
    }
  })

  it('refuses the sign-in routes from another origin too', () => {
    for (const path of ['/api/auth/login', '/api/auth/logout', '/api/auth/challenge']) {
      expect(refusal(request({ path, origin: 'https://other.bancwr.example' }), SITE)?.statusCode).toBe(403)
    }
  })

  it('falls back to Sec-Fetch-Site when there is no Origin', () => {
    expect(refusal(request({ secFetchSite: 'same-origin' }), SITE)).toBeUndefined()
    for (const site of ['same-site', 'cross-site', 'none']) {
      expect(refusal(request({ secFetchSite: site }), SITE)?.data.reason).toBe('bad_origin')
    }
  })

  it('lets a request with neither header through, since no browser page sent it', () => {
    expect(refusal(request({ contentType: 'application/json', hasBody: true }), SITE)).toBeUndefined()
  })

  it('never checks a request that changes nothing', () => {
    for (const method of ['GET', 'HEAD', 'OPTIONS']) {
      expect(refusal(request({ method, origin: 'https://evil.example' }), SITE)).toBeUndefined()
    }
  })

  it('refuses a body for the bunker that is not declared JSON, as a form would send', () => {
    for (const contentType of [undefined, 'text/plain', 'application/x-www-form-urlencoded', 'multipart/form-data; boundary=x']) {
      expect(refusal(request({ origin: SITE, contentType, hasBody: true }), SITE)?.statusCode).toBe(415)
    }
    expect(refusal(request({ path: '/API/BUNKER/team', origin: SITE, contentType: 'text/plain', hasBody: true }), SITE)?.statusCode).toBe(415)
  })

  it('needs no content type for a request with no body', () => {
    expect(refusal(request({ origin: SITE, hasBody: false }), SITE)).toBeUndefined()
  })
})
