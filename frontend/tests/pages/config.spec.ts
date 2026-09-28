import { describe, it, expect } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import Config from '../../app/pages/config.vue'
import type { ConfigResponse } from '#shared/types/bunker'

// The real ConfigResponse: the backend never returns the nsec.
const mockConfig: ConfigResponse = { pubkey: 'npub1test', nsec_file: null }

describe('Config page', () => {
  it('renders form inputs with initial data', async () => {
    registerEndpoint('/api/bunker/config', () => mockConfig)

    const component = await mountSuspended(Config)

    expect(component.text()).toContain('Bunker Configuration')
    expect(component.text()).toContain('Current Pubkey')

    const inputs = component.findAll('input')
    // One for pubkey (disabled), one for nsec, one for nsecFile
    expect(inputs.length).toBe(3)

    expect(inputs[0]?.element.value).toBe('npub1test')
    // The nsec field never starts filled in.
    expect(inputs[1]?.element.value).toBe('')
  })

  it('binds inputs to state', async () => {
    registerEndpoint('/api/bunker/config', () => mockConfig)

    const component = await mountSuspended(Config)
    const [, second, third] = component.findAll('input')

    await second?.setValue('new-nsec')
    await third?.setValue('/path/to/nsec')

    expect(second?.element.value).toBe('new-nsec')
    expect(third?.element.value).toBe('/path/to/nsec')
  })
})
