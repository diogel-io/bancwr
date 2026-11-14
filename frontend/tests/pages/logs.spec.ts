import { describe, it, expect, beforeEach } from 'vitest'
import { createError } from 'h3'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import type { LogEntry } from '#shared/types/bunker'
import Logs from '../../app/pages/logs.vue'
import { clearNuxtData } from '#imports'
import { LOG_PAGE_SIZE } from '../../app/utils/logs'

// The real LogEntry shape from backend/src/server.rs.
const mockLogs: LogEntry[] = [
  { id: '1', event_id: 'e1', pubkey: 'npub1alice', event_kind: 1, timestamp: '2025-01-01T10:00:00Z' }
]

describe('Logs page', () => {
  beforeEach(async () => {
    await clearNuxtData()
  })

  it('renders logs page header', async () => {
    registerEndpoint('/api/bunker/logs', {
      method: 'GET',
      handler: () => mockLogs
    })

    const component = await mountSuspended(Logs)

    expect(component.text()).toContain('Signing Activity Log')
    // The data should be passed to ActivityLog
    expect(component.text()).toContain('npub1alice')
  })

  it('handles empty logs', async () => {
    registerEndpoint('/api/bunker/logs', {
      method: 'GET',
      handler: () => []
    })

    const component = await mountSuspended(Logs)
    // It should render ActivityLog which shows "No data"
    expect(component.text()).toContain('No data')
  })

  it('explains a bunker 403 rather than breaking (#26)', async () => {
    registerEndpoint('/api/bunker/logs', () => {
      throw createError({ status: 403, data: { error: 'forbidden' } })
    })

    const component = await mountSuspended(Logs)

    expect(component.find('[data-testid="forbidden-notice"]').text()).toContain('Your role no longer allows this page')
  })

  it('loads older entries until a short page, continuing from the last row (#96)', async () => {
    const page = (start: number, count: number): LogEntry[] => Array.from({ length: count }, (_, i) => ({
      id: `id-${start + i}`, event_id: `ev-${start + i}`, pubkey: 'cc', event_kind: 1, timestamp: new Date(Date.UTC(2026, 0, 1) - (start + i) * 1000).toISOString()
    }))
    const befores: (string | null)[] = []
    registerEndpoint('/api/bunker/logs', (event) => {
      const before = new URL(event.path, 'http://x').searchParams.get('before')
      befores.push(before)
      return before ? page(LOG_PAGE_SIZE, 3) : page(0, LOG_PAGE_SIZE)
    })

    const component = await mountSuspended(Logs)
    expect(component.findAll('tbody tr')).toHaveLength(LOG_PAGE_SIZE)
    await component.find('[data-testid="log-load-older"]').trigger('click')
    await new Promise(resolve => setTimeout(resolve, 50))

    expect(component.findAll('tbody tr')).toHaveLength(LOG_PAGE_SIZE + 3)
    const last = page(0, LOG_PAGE_SIZE).at(-1)!
    expect(befores.at(-1)).toBe(`${last.timestamp}|${last.id}`)
    expect(component.find('[data-testid="log-load-older"]').exists()).toBe(false)
  })

  it('offers no older entries when the first page is short (#96)', async () => {
    registerEndpoint('/api/bunker/logs', () => mockLogs)
    const component = await mountSuspended(Logs)
    expect(component.find('[data-testid="log-load-older"]').exists()).toBe(false)
  })

  it('links the export to the chosen range, and refuses a reversed one (#96)', async () => {
    registerEndpoint('/api/bunker/logs', () => mockLogs)
    const component = await mountSuspended(Logs)

    const link = () => component.find('[data-testid="log-export-download"]')
    expect(link().attributes('href')).toMatch(/^\/api\/bunker\/logs\/export\?format=csv&from=\d{4}-\d{2}-\d{2}&to=\d{4}-\d{2}-\d{2}$/u)
    expect(link().attributes('download')).toBeDefined()

    await component.find('input[data-testid="log-export-from"]').setValue('2026-06-02')
    await component.find('input[data-testid="log-export-to"]').setValue('2026-06-01')
    expect(component.find('[data-testid="log-export-error"]').text()).toBe('The from date must not be after the to date.')
    expect(link().attributes('href')).toBeUndefined()
  })
})
