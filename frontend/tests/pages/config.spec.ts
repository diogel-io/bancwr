import { describe, it, expect, beforeEach } from 'vitest'
import { createError } from 'h3'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import Config from '../../app/pages/config.vue'
import type { ConfigResponse } from '#shared/types/bunker'
import { clearNuxtData } from '#imports'

// The real ConfigResponse: the backend never returns the nsec.
const fromEnv: ConfigResponse = { pubkey: 'npub1test', nsec_file: null }
const fromFile: ConfigResponse = { pubkey: 'npub1test', nsec_file: '/run/secrets/bunker_nsec' }

describe('Config page', () => {
  // useFetch caches by key across mounts; each test must fetch its own response.
  beforeEach(async () => {
    await clearNuxtData()
  })

  it('shows the bunker public key', async () => {
    registerEndpoint('/api/bunker/config', () => fromEnv)

    const component = await mountSuspended(Config)

    expect(component.text()).toContain('Bunker Configuration')
    expect(component.text()).toContain('Current Pubkey')
    expect(component.text()).toContain('npub1test')
  })

  it('names the environment variable when the key does not come from a file', async () => {
    registerEndpoint('/api/bunker/config', () => fromEnv)

    const component = await mountSuspended(Config)

    expect(component.text()).toContain('Environment variable: BUNKER_NSEC')
  })

  it('shows the file path when the key comes from a file', async () => {
    registerEndpoint('/api/bunker/config', () => fromFile)

    const component = await mountSuspended(Config)

    expect(component.text()).toContain('File: /run/secrets/bunker_nsec')
  })

  it('is read-only: no inputs, no save button, and it explains how to change the key', async () => {
    registerEndpoint('/api/bunker/config', () => fromEnv)

    const component = await mountSuspended(Config)

    expect(component.findAll('input')).toHaveLength(0)
    expect(component.findAll('button').filter(b => /save/i.test(b.text()))).toHaveLength(0)
    expect(component.text()).toContain('To change the signing key, set BUNKER_NSEC_FILE')
  })

  it('never sends anything to the config endpoint', async () => {
    const methods: string[] = []
    registerEndpoint('/api/bunker/config', (event) => {
      methods.push(event.method)
      return fromEnv
    })

    await mountSuspended(Config)

    expect(methods.length).toBeGreaterThan(0)
    expect(methods.every(m => m === 'GET')).toBe(true)
  })

  it('explains a bunker 403 rather than breaking (#26)', async () => {
    registerEndpoint('/api/bunker/config', () => {
      throw createError({ status: 403, data: { error: 'forbidden' } })
    })

    const component = await mountSuspended(Config)

    expect(component.find('[data-testid="forbidden-notice"]').text()).toContain('Your role no longer allows this page')
  })
})
