import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import type { BunkerHealth as Health } from '~/composables/useBunkerHealth'
import { NO_ANSWER } from '~/composables/useBunkerHealth'
import BunkerHealth from '~/components/BunkerHealth.vue'
import { useState } from '#imports'

// The popover needs the provider UApp installs in app.vue.
const InApp = defineComponent({ render: () => h(UApp, () => h(BunkerHealth)) })

// Left unanswered, so a mount with nothing known stays unknown rather than racing a fetch.
registerEndpoint('/api/bunker/status', () => new Promise(() => {}))

const relays = {
  name: 'relays',
  status: 'warn' as const,
  detail: '1 of 2 relays connected. Not connected: wss://b.example',
  relays: [{ url: 'wss://a.example', connected: true }, { url: 'wss://b.example', connected: false }]
}

const cases: [string, Health, string, string][] = [
  ['unknown', { state: 'unknown', checks: [] }, 'Checking…', 'bg-neutral-400'],
  ['healthy', { state: 'healthy', checks: [], checkedAt: Date.now() }, 'Healthy', 'bg-success'],
  ['degraded', { state: 'degraded', checks: [relays], checkedAt: Date.now() }, 'Degraded', 'bg-warning'],
  ['unhealthy', { state: 'unhealthy', checks: [{ name: 'database', status: 'fail', detail: 'The database did not answer. See the bunker log.' }], checkedAt: Date.now() }, 'Down', 'bg-error'],
  ['no answer', { state: 'unhealthy', checks: [NO_ANSWER], checkedAt: Date.now() }, 'Down', 'bg-error']
]

let mounted: { unmount: () => void } | undefined

async function mountWith(health: Health) {
  useState('bunker-health').value = health
  const component = await mountSuspended(InApp, { attachTo: document.body })
  mounted = component
  return component
}

async function openDetail(component: Awaited<ReturnType<typeof mountWith>>) {
  await component.find('[data-testid="bunker-health"]').trigger('click')
  await flushPromises()
  return document.body.querySelector('[data-testid="bunker-health-detail"]')
}

describe('BunkerHealth', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
  })

  afterEach(() => {
    // Stops its poll.
    mounted?.unmount()
    mounted = undefined
  })

  for (const [name, health, label, colour] of cases) {
    it(`shows ${name} by text, accessible name and colour`, async () => {
      const component = await mountWith(health)
      const button = component.find('[data-testid="bunker-health"]')

      expect(button.text()).toContain(label)
      expect(button.attributes('aria-label')).toBe(`Bunker health: ${label}. Show details`)
      expect(button.find('span').classes()).toContain(colour)
      expect(component.find('[role="status"]').text()).toBe(`Bunker health: ${label}`)
    })
  }

  it('never shows green before the first answer', async () => {
    const component = await mountWith({ state: 'unknown', checks: [] })
    expect(component.find('[data-testid="bunker-health"]').text()).not.toContain('Healthy')
  })

  it('names what is degraded, relay by relay, without leaving the page', async () => {
    const component = await mountWith(cases[2]![1])
    const detail = await openDetail(component)

    expect(detail?.textContent).toContain('Bunker health: Degraded')
    expect(detail?.textContent).toContain('1 of 2 relays connected. Not connected: wss://b.example')
    expect(detail?.textContent).toContain('wss://a.example')
    expect(detail?.textContent).toContain('not connected')
    expect(detail?.textContent).toMatch(/Last checked \d+ s ago/)
  })

  it('says the bunker did not answer when the request failed', async () => {
    const component = await mountWith(cases[4]![1])
    const detail = await openDetail(component)

    expect(detail?.textContent).toContain('The bunker did not answer')
  })
})
