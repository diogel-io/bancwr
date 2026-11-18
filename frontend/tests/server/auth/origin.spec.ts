import { describe, it, expect } from 'vitest'
import { hasBody, isSafeMethod, jsonBodyRefusal, refusal, type IncomingRequest } from '../../../server/utils/auth/origin'

const SITE = 'https://bancwr.example'

function request(overrides: Partial<IncomingRequest>): IncomingRequest {
  return { method: 'POST', sessionCookie: true, ...overrides }
}

describe('which requests the console accepts, by where they came from', () => {
  it('lets the console\'s own requests through', () => {
    for (const method of ['POST', 'PUT', 'PATCH', 'DELETE']) {
      expect(refusal(request({ method, origin: SITE }), SITE)).toBeUndefined()
    }
  })

  it('refuses a request that changes something from another origin, on the same site or not', () => {
    for (const origin of ['https://evil.example', 'https://other.bancwr.example', 'http://bancwr.example', `${SITE}/`, 'null']) {
      for (const method of ['POST', 'PUT', 'PATCH', 'DELETE']) {
        for (const sessionCookie of [true, false]) {
          expect(refusal(request({ method, origin, sessionCookie }), SITE)?.data)
            .toEqual({ error: 'forbidden', reason: 'bad_origin' })
        }
      }
    }
  })

  it('goes by Origin when there is one, whatever Sec-Fetch-Site says', () => {
    expect(refusal(request({ origin: 'https://other.bancwr.example', secFetchSite: 'same-origin' }), SITE)?.statusCode).toBe(403)
    expect(refusal(request({ origin: SITE, secFetchSite: 'same-site' }), SITE)).toBeUndefined()
  })

  it('falls back to Sec-Fetch-Site when there is no Origin', () => {
    expect(refusal(request({ secFetchSite: 'same-origin' }), SITE)).toBeUndefined()
    for (const site of ['same-site', 'cross-site', 'none']) {
      expect(refusal(request({ secFetchSite: site }), SITE)?.data.reason).toBe('bad_origin')
    }
  })

  it('lets a request with neither header and no session cookie through, as an operator\'s script sends', () => {
    for (const method of ['POST', 'DELETE']) {
      expect(refusal(request({ method, sessionCookie: false }), SITE)).toBeUndefined()
    }
  })

  it('refuses a request with neither header that carries the session cookie', () => {
    for (const method of ['POST', 'PUT', 'PATCH', 'DELETE']) {
      expect(refusal(request({ method, sessionCookie: true }), SITE)?.data)
        .toEqual({ error: 'forbidden', reason: 'bad_origin' })
    }
  })

  it('refuses the sign-in routes from another origin too', () => {
    // The check does not depend on the path: it is the same for every route.
    expect(refusal(request({ origin: 'https://other.bancwr.example', sessionCookie: false }), SITE)?.statusCode).toBe(403)
  })

  it('never checks a request that changes nothing', () => {
    for (const method of ['GET', 'HEAD', 'OPTIONS', 'get']) {
      expect(isSafeMethod(method)).toBe(true)
      expect(refusal(request({ method, origin: 'https://evil.example' }), SITE)).toBeUndefined()
    }
    for (const method of ['POST', 'PUT', 'PATCH', 'DELETE', 'post', 'PROPFIND']) {
      expect(isSafeMethod(method)).toBe(false)
    }
  })
})

describe('whether a request carries a body', () => {
  it('has none without a length or with a length of 0', () => {
    expect(hasBody(undefined, undefined)).toBe(false)
    expect(hasBody('0', undefined)).toBe(false)
  })

  it('has one with any other length, or any transfer encoding', () => {
    for (const length of ['1', '42', '00', ' 0', '']) {
      expect(hasBody(length, undefined)).toBe(true)
    }
    for (const encoding of ['chunked', 'gzip, chunked', '']) {
      expect(hasBody(undefined, encoding)).toBe(true)
      expect(hasBody('0', encoding)).toBe(true)
    }
  })
})

describe('which bodies the proxy forwards to the bunker', () => {
  it('forwards a body declared JSON, with or without parameters', () => {
    for (const contentType of ['application/json', 'application/json; charset=utf-8', 'Application/JSON']) {
      expect(jsonBodyRefusal({ hasBody: true, contentType })).toBeUndefined()
    }
  })

  it('refuses a body that is not declared JSON, as a form would send', () => {
    for (const contentType of [
      undefined, '', 'text/plain', 'application/x-www-form-urlencoded', 'multipart/form-data; boundary=x',
      'application/json-foo', 'text/plain; application/json', 'application/jsonp'
    ]) {
      expect(jsonBodyRefusal({ hasBody: true, contentType })?.statusCode).toBe(415)
    }
  })

  it('needs no content type for a request with no body', () => {
    expect(jsonBodyRefusal({ hasBody: false })).toBeUndefined()
    expect(jsonBodyRefusal({ hasBody: false, contentType: 'text/plain' })).toBeUndefined()
  })
})
