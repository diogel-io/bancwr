import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { registerEndpoint } from '@nuxt/test-utils/runtime'
import { createError } from 'h3'
import type { BunkerStatus } from '#shared/types/bunker'
import { HEALTH_POLL_MS, NO_ANSWER, subscribeToBunkerHealth, useBunkerHealth } from '~/composables/useBunkerHealth'
import { useState } from '#imports'

const degraded: BunkerStatus = {
  status: 'degraded',
  pubkey: 'npub1test',
  version: '0.1.0',
  checks: [{ name: 'relays', status: 'warn', detail: '1 of 2 relays connected. Not connected: wss://b.example' }]
}

// What the status endpoint answers next: a response, or an HTTP status to fail with.
let answer: BunkerStatus | number = degraded
registerEndpoint('/api/bunker/status', () => {
  if (typeof answer === 'number') throw createError({ status: answer })
  return answer
})

describe('useBunkerHealth', () => {
  beforeEach(() => {
    useState('bunker-health').value = { state: 'unknown', checks: [] }
    answer = degraded
  })

  it('is unknown before the first answer, never green by default', () => {
    expect(useBunkerHealth().health.value.state).toBe('unknown')
  })

  it('takes the state and checks from the bunker', async () => {
    const { health, refresh } = useBunkerHealth()
    await refresh()
    expect(health.value.state).toBe('degraded')
    expect(health.value.checks).toEqual(degraded.checks)
    expect(health.value.checkedAt).toBeTypeOf('number')
  })

  it('is red when the bunker does not answer, saying so', async () => {
    answer = 502
    const { health, refresh } = useBunkerHealth()
    await refresh()
    expect(health.value.state).toBe('unhealthy')
    expect(health.value.checks).toEqual([NO_ANSWER])
  })

  it('keeps the last state on a 401 or 403: that is the session, not the bunker', async () => {
    const { health, refresh } = useBunkerHealth()
    await refresh()
    for (const status of [401, 403]) {
      answer = status
      await refresh()
      expect(health.value.state, String(status)).toBe('degraded')
    }
  })

  it('fetches once only if nothing has answered yet', async () => {
    const { health, ensure } = useBunkerHealth()
    await ensure()
    expect(health.value.state).toBe('degraded')
    answer = { ...degraded, status: 'healthy', checks: [] }
    await ensure()
    expect(health.value.state).toBe('degraded')
  })
})

describe('polling', () => {
  let visibility: DocumentVisibilityState = 'visible'

  beforeEach(() => {
    vi.useFakeTimers()
    visibility = 'visible'
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  function setVisibility(state: DocumentVisibilityState) {
    visibility = state
    document.dispatchEvent(new Event('visibilitychange'))
  }

  it('polls every 30 s, once however many subscribe, and stops when the last one leaves', () => {
    const refresh = vi.fn(async () => {})
    const first = subscribeToBunkerHealth(refresh)
    const second = subscribeToBunkerHealth(refresh)

    vi.advanceTimersByTime(HEALTH_POLL_MS)
    expect(refresh).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(HEALTH_POLL_MS)
    expect(refresh).toHaveBeenCalledTimes(2)

    first()
    first() // Twice is harmless.
    vi.advanceTimersByTime(HEALTH_POLL_MS)
    expect(refresh).toHaveBeenCalledTimes(3)

    second()
    vi.advanceTimersByTime(HEALTH_POLL_MS * 3)
    expect(refresh).toHaveBeenCalledTimes(3)
  })

  it('pauses while the tab is hidden, and checks at once on return', () => {
    const refresh = vi.fn(async () => {})
    const unsubscribe = subscribeToBunkerHealth(refresh)

    setVisibility('hidden')
    vi.advanceTimersByTime(HEALTH_POLL_MS * 4)
    expect(refresh).not.toHaveBeenCalled()

    setVisibility('visible')
    expect(refresh).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(HEALTH_POLL_MS)
    expect(refresh).toHaveBeenCalledTimes(2)

    unsubscribe()
    setVisibility('hidden')
    setVisibility('visible')
    expect(refresh).toHaveBeenCalledTimes(2)
  })
})
