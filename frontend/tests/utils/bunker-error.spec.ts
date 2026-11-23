// Reading the bunker's refusal through the proxy, which wraps it in Nuxt's error body.
import { describe, it, expect } from 'vitest'
import { bunkerRefusal } from '~/utils/bunker-error'

describe('bunkerRefusal', () => {
  it('reads the bunker\'s answer from under the proxy\'s error, not its status text', () => {
    const failure = { data: { statusCode: 409, statusMessage: 'Conflict', message: 'Conflict', data: { error: 'nip46_disabled', message: 'NIP-46 is turned off' } } }
    expect(bunkerRefusal(failure)).toEqual({ error: 'nip46_disabled', message: 'NIP-46 is turned off' })
  })

  it('shows nothing when the proxy\'s error carries no answer from the bunker', () => {
    expect(bunkerRefusal({ data: { statusCode: 500, statusMessage: 'Internal Server Error', message: 'Internal Server Error' } })).toEqual({})
  })

  it('reads an answer that is not wrapped as it is', () => {
    expect(bunkerRefusal({ data: { error: 'event_changed', message: 'Reload' } })).toEqual({ error: 'event_changed', message: 'Reload' })
  })

  it('copes with a failure that has no body', () => {
    expect(bunkerRefusal(new Error('network'))).toEqual({})
    expect(bunkerRefusal(undefined)).toEqual({})
  })
})
