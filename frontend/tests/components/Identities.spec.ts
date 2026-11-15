// Identities in the console (#103): the Config section that lists and renames them, the team page's
// "May sign as" for a member, the add-member form's choice, and the log's "Signed as" column, each
// shown only once the bunker holds more than one identity.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { readBody } from 'h3'
import { UApp } from '#components'
import type { BunkerIdentity, LogEntry } from '#shared/types/bunker'
import IdentitiesSection from '~/components/identities/IdentitiesSection.vue'
import IdentityAccess from '~/components/identities/IdentityAccess.vue'
import ActivityLog from '~/components/ActivityLog.vue'
import Team from '~/pages/team/index.vue'
import { signInAs } from '../helpers/session'
import { clearNuxtData } from '#imports'

const PRIMARY = 'e'.repeat(64)
const BRAND = '9'.repeat(64)
const SAM = '5'.repeat(64)

const state = vi.hoisted(() => ({
  identities: [] as unknown[],
  calls: [] as { method: string, path: string, body?: unknown }[]
}))

const two = (): BunkerIdentity[] => [
  { pubkey: PRIMARY, npub: 'npub1primary', label: 'Primary', is_primary: true, available: true, members: [SAM] },
  { pubkey: BRAND, npub: 'npub1brand', label: 'Brand B', is_primary: false, available: false, members: [] }
]

registerEndpoint('/api/bunker/identities', () => state.identities)
registerEndpoint(`/api/bunker/identities/${BRAND}`, {
  method: 'PATCH',
  handler: async (event) => {
    state.calls.push({ method: 'PATCH', path: BRAND, body: await readBody(event) })
    return { success: true }
  }
})
for (const method of ['PUT', 'DELETE'] as const) {
  registerEndpoint(`/api/bunker/identities/${BRAND}/members/${SAM}`, {
    method,
    handler: () => {
      state.calls.push({ method, path: `${BRAND}/${SAM}` })
      return { success: true }
    }
  })
}
registerEndpoint('/api/bunker/team', {
  method: 'GET',
  handler: () => []
})
registerEndpoint('/api/bunker/team', {
  method: 'POST',
  handler: async (event) => {
    state.calls.push({ method: 'POST', path: 'team', body: await readBody(event) })
    return { success: true, message: 'ok' }
  }
})

const inApp = (component: unknown, props: Record<string, unknown> = {}) =>
  defineComponent({ render: () => h(UApp, () => h(component as never, props)) })

// Unmounted after each test: they share the identities' useFetch key.
let mounted: { unmount: () => void }[] = []
async function mount(component: ReturnType<typeof defineComponent> | typeof ActivityLog, options: Record<string, unknown> = {}) {
  const wrapper = await mountSuspended(component as never, options)
  mounted.push(wrapper)
  await flushPromises()
  return wrapper
}

describe('Identities', () => {
  afterEach(() => {
    for (const wrapper of mounted) wrapper.unmount()
    mounted = []
  })

  beforeEach(async () => {
    document.body.innerHTML = ''
    await clearNuxtData()
    signInAs('administrator')
    Object.assign(state, { identities: [], calls: [] })
  })

  it('lists each identity on the Config page, marks the primary and one not loaded yet, and renames', async () => {
    state.identities = two()
    const component = await mount(inApp(IdentitiesSection), { attachTo: document.body })
    const rows = component.findAll('[data-identity]')
    expect(rows.map(r => r.attributes('data-identity'))).toEqual([PRIMARY, BRAND])
    expect(rows[0]!.text()).toContain('Primary')
    expect(rows[1]!.find('[data-testid="identity-not-loaded"]').text()).toBe('Loads when the bunker restarts')
    expect(component.text()).toContain('bunker import-key --add')

    await rows[1]!.find('button').trigger('click')
    await component.find(`[data-identity="${BRAND}"] input`).setValue('Brand Bee')
    await component.findAll(`[data-identity="${BRAND}"] button`).find(b => b.text() === 'Save')!.trigger('click')
    await vi.waitFor(() => expect(state.calls).toEqual([{ method: 'PATCH', path: BRAND, body: { label: 'Brand Bee' } }]))
  })

  it('grants and removes a member\'s identities on their team page, only with several', async () => {
    state.identities = two()
    const component = await mount(inApp(IdentityAccess, { member: SAM, administrator: false }), { attachTo: document.body })
    const brand = component.find(`[data-identity="${BRAND}"]`)
    const primary = component.find(`[data-identity="${PRIMARY}"]`)
    expect(primary.attributes('data-state') ?? primary.find('[data-state]').attributes('data-state')).toBe('checked')
    await (brand.element.tagName === 'BUTTON' ? brand : brand.find('button')).trigger('click')
    await vi.waitFor(() => expect(state.calls).toEqual([{ method: 'PUT', path: `${BRAND}/${SAM}` }]))

    // With one identity, nothing is shown.
    component.unmount()
    mounted = []
    state.identities = two().slice(0, 1)
    await clearNuxtData()
    const single = await mount(inApp(IdentityAccess, { member: SAM, administrator: false }))
    expect(single.find('[data-testid="identity-access"]').exists()).toBe(false)
  })

  it('offers the identities when adding a member, the primary preselected, and sends them', async () => {
    state.identities = two()
    const component = await mount(inApp(Team), { attachTo: document.body })
    await vi.waitFor(() => expect(component.find('[data-testid="team-member-identities"]').exists()).toBe(true))
    await component.find('#team-member-name').setValue('Pat')
    await component.find('#team-member-pubkey').setValue('npub1pat')
    await component.findAll('button').find(b => b.text().includes('Add Member') || b.text() === 'Add')!.trigger('click')
    await vi.waitFor(() => expect(state.calls.find(c => c.path === 'team')).toBeTruthy())
    expect(state.calls.find(c => c.path === 'team')!.body).toMatchObject({ name: 'Pat', identities: [PRIMARY] })
  })

  it('names the identity in the log only once there are several', async () => {
    const rows: LogEntry[] = [{ id: '1', event_id: 'e1', pubkey: 'a'.repeat(64), event_kind: 1, timestamp: '2025-01-01T10:00:00Z', member_pubkey: SAM, member_name: 'Sam', connection_id: 'c1', identity_pubkey: BRAND }]
    state.identities = two()
    const several = await mount(ActivityLog, { props: { rows } })
    await vi.waitFor(() => expect(several.find('[data-testid="log-identity"]').text()).toBe('Brand B'))

    several.unmount()
    mounted = []
    state.identities = two().slice(0, 1)
    await clearNuxtData()
    const one = await mount(ActivityLog, { props: { rows } })
    expect(one.find('[data-testid="log-identity"]').exists()).toBe(false)
  })
})
