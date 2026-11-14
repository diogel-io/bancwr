// The log retention card on the Config page (#96): the policy in words, the floors, and saving.
import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { readBody, setResponseStatus } from 'h3'
import LogRetention from '~/components/LogRetention.vue'
import type { LogRetention as Policy, LogRetentionResponse } from '#shared/types/bunker'
import { clearNuxtData } from '#imports'

let current: LogRetentionResponse
let puts: Policy[]
let refusal: { error: string, message: string } | undefined

registerEndpoint('/api/bunker/logs/retention', async (event) => {
  if (event.method === 'PUT') {
    const body = await readBody<Policy>(event)
    puts.push(body)
    if (refusal) {
      setResponseStatus(event, 400)
      return refusal
    }
    current = { ...current, ...body }
  }
  return current
})

const input = (component: Awaited<ReturnType<typeof mountSuspended>>, id: string) => component.find(`input[data-testid="${id}"]`)
/** A save goes through the mocked endpoint, which reads the body asynchronously. */
const settle = async () => {
  await flushPromises()
  await new Promise(resolve => setTimeout(resolve, 50))
}
const save = (component: Awaited<ReturnType<typeof mountSuspended>>) => component.find('[data-testid="log-retention-save"]')

describe('LogRetention', () => {
  beforeEach(async () => {
    await clearNuxtData()
    current = { max_age_days: null, max_rows: null, min_age_days: 30, min_rows: 10000 }
    puts = []
    refusal = undefined
  })

  it('says entries are kept indefinitely until a policy is set', async () => {
    const component = await mountSuspended(LogRetention)
    expect(component.find('[data-testid="log-retention-current"]').text()).toBe('Kept indefinitely')
    expect(save(component).attributes('disabled')).toBeDefined()
  })

  it('saves a policy, empty fields meaning no limit, and shows it', async () => {
    const component = await mountSuspended(LogRetention)

    await input(component, 'log-retention-days').setValue('90')
    await component.find('form').trigger('submit')
    await settle()

    expect(puts).toEqual([{ max_age_days: 90, max_rows: null }])
    expect(component.find('[data-testid="log-retention-current"]').text()).toBe('Kept for 90 days')
    expect(component.find('[data-testid="log-retention-saved"]').exists()).toBe(true)
  })

  it('refuses a value under the floor before sending it', async () => {
    const component = await mountSuspended(LogRetention)

    await input(component, 'log-retention-rows').setValue('500')

    expect(component.text()).toContain('At least 10,000 rows, or leave it empty.')
    expect(save(component).attributes('disabled')).toBeDefined()
    await component.find('form').trigger('submit')
    await settle()
    expect(puts).toEqual([])
  })

  it('shows the bunker\'s refusal', async () => {
    refusal = { error: 'invalid_max_age', message: 'Keep logs for at least 30 days.' }
    const component = await mountSuspended(LogRetention)

    await input(component, 'log-retention-days').setValue('45')
    await component.find('form').trigger('submit')
    await settle()

    expect(component.find('[data-testid="log-retention-save-error"]').text()).toBe('Keep logs for at least 30 days.')
  })
})
