import { describe, it, expect, beforeEach } from 'vitest'
import { createError } from 'h3'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import Config from '../../app/pages/config.vue'
import type { ConfigResponse } from '#shared/types/bunker'
import { clearNuxtData } from '#imports'

// The real ConfigResponse: the backend never returns the key, or where it is stored (#94).
const fromEnv: ConfigResponse = { pubkey: 'npub1test', key_storage: 'encrypted' }

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

  it('says the key is encrypted at rest, and names no plain key or path (#94)', async () => {
    registerEndpoint('/api/bunker/config', () => fromEnv)

    const component = await mountSuspended(Config)

    expect(component.find('[data-testid="key-storage"]').text()).toBe('Encrypted at rest')
    expect(component.text()).not.toContain('BUNKER_NSEC')
    expect(component.text()).not.toContain('/run/secrets')
  })

  it('is read-only: no inputs, no save button, and it explains how to change the key', async () => {
    registerEndpoint('/api/bunker/config', () => fromEnv)

    const component = await mountSuspended(Config)

    expect(component.findAll('input')).toHaveLength(0)
    expect(component.findAll('button').filter(b => /save/i.test(b.text()))).toHaveLength(0)
    expect(component.text()).toContain('It is imported once with bunker import-key')
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

  it('shows the bunker\'s own relays below the key (#78), read-only when NIP46_RELAYS sets them', async () => {
    registerEndpoint('/api/bunker/config', () => fromEnv)
    registerEndpoint('/api/bunker/relays', () => ({ source: 'environment', nip46_enabled: true, relays: [{ url: 'wss://env.example', connected: true }] }))

    const component = await mountSuspended(Config)

    const section = component.find('[data-testid="bunker-relays-section"]')
    expect(section.text()).toContain('Bunker relays')
    expect(section.text()).toContain('wss://env.example')
    expect(section.find('[data-testid="bunker-relays-read-only"]').exists()).toBe(true)
    expect(component.findAll('input')).toHaveLength(0)
  })
})
