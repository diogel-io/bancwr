import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { clearNuxtData } from '#imports'
import MetricsCards from '~/components/MetricsCards.vue'

describe('MetricsCards', () => {
  // useFetch caches on its key, so without this each test would see the first test's payload.
  beforeEach(async () => {
    await clearNuxtData()
  })

  it('shows the counts the bunker reports', async () => {
    registerEndpoint('/api/bunker/metrics', () => ({
      http_requests: 42,
      nip46_connections: 3,
      total_signatures: 1234
    }))

    const component = await mountSuspended(MetricsCards)

    expect(component.text()).toContain('1234')
    expect(component.text()).toContain('Total Signatures')
    expect(component.text()).toContain('3')
    expect(component.text()).toContain('NIP-46 Connections')
    expect(component.text()).toContain('42')
    expect(component.text()).toContain('HTTP Requests')
  })

  it('reports a signing total beyond the 100 rows the logs endpoint returns', async () => {
    // The dashboard used to derive this from logs.length, which the backend caps at 100.
    registerEndpoint('/api/bunker/metrics', () => ({
      http_requests: 0,
      nip46_connections: 0,
      total_signatures: 5000
    }))

    const component = await mountSuspended(MetricsCards)

    expect(component.text()).toContain('5000')
  })

  it('falls back to zero when the bunker is unreachable', async () => {
    registerEndpoint('/api/bunker/metrics', () => {
      throw new Error('unreachable')
    })

    const component = await mountSuspended(MetricsCards)

    expect(component.text()).toContain('Total Signatures')
    expect(component.text()).toContain('0')
  })
})
