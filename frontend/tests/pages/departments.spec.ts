// The Departments page (#147): an administrator sees every department, with its members and leads,
// and the administrator only identities apart; a lead sees the departments they lead. Each change
// is sent to the bunker, which decides.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { createError, readBody } from 'h3'
import { UApp } from '#components'
import type { BunkerIdentity, Role } from '#shared/types/bunker'
import { signInAs } from '../helpers/session'
import Departments from '~/pages/departments.vue'
import { clearNuxtData } from '#imports'

// signInAs() signs in as 'a'.repeat(64): Pam here.
const PAM = 'a'.repeat(64)
const PETER = 'b'.repeat(64)
const JAMES = 'c'.repeat(64)
const SALES = '7'.repeat(64)
const SUPPORT = '6'.repeat(64)
const ROOT = '5'.repeat(64)

const state = vi.hoisted(() => ({
  identities: [] as unknown[],
  calls: [] as { method: string, path: string, body?: unknown }[],
  refuse: undefined as string | undefined
}))

const all = (): BunkerIdentity[] => [
  { pubkey: SALES, npub: 'npub1sales', label: 'Sales', is_primary: false, available: true, members: [PAM, PETER], leads: [PAM], my_role: 'lead' },
  { pubkey: SUPPORT, npub: 'npub1support', label: 'Support', is_primary: false, available: true, members: [JAMES], leads: [] },
  { pubkey: ROOT, npub: 'npub1root', label: 'Company root', is_primary: false, available: true, members: [], leads: [], admin_only: true }
]

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))
registerEndpoint('/api/bunker/identities', () => state.identities)
registerEndpoint('/api/bunker/team', {
  method: 'GET',
  handler: () => [
    { id: '1', name: 'Pam', pubkey: PAM, npub: 'npub1pam', role: 'signer' },
    { id: '2', name: 'Peter', pubkey: PETER, npub: 'npub1peter', role: 'signer' },
    { id: '3', name: 'James', pubkey: JAMES, npub: 'npub1james', role: 'signer' }
  ]
})
registerEndpoint('/api/bunker/team', {
  method: 'POST',
  handler: async (event) => {
    if (state.refuse) throw createError({ statusCode: 403, statusMessage: 'Forbidden', data: { error: state.refuse, message: 'no' } })
    state.calls.push({ method: 'POST', path: 'team', body: await readBody(event) })
    return { success: true }
  }
})
for (const identity of [SALES, SUPPORT]) {
  for (const member of [PAM, PETER, JAMES]) {
    for (const method of ['PUT', 'DELETE'] as const) {
      registerEndpoint(`/api/bunker/identities/${identity}/members/${member}`, {
        method,
        handler: async (event) => {
          const body = method === 'PUT' ? await readBody(event) : undefined
          state.calls.push({ method, path: `${identity}/${member}`, ...(body ? { body } : {}) })
          return { success: true }
        }
      })
    }
  }
}
for (const identity of [SUPPORT, ROOT]) {
  registerEndpoint(`/api/bunker/identities/${identity}`, {
    method: 'PATCH',
    handler: async (event) => {
      state.calls.push({ method: 'PATCH', path: identity, body: await readBody(event) })
      return { success: true }
    }
  })
}

const InApp = defineComponent({ render: () => h(UApp, () => h(Departments)) })
let mounted: { unmount: () => void }[] = []

async function mount(role: Role) {
  signInAs(role)
  const component = await mountSuspended(InApp, { attachTo: document.body })
  mounted.push(component)
  await flushPromises()
  return component
}

/** Opens a select by its label and picks an option by its text. */
async function pick(component: Awaited<ReturnType<typeof mount>>, label: string, option: string) {
  await component.get(`button[aria-label="${label}"]`).trigger('click')
  await flushPromises()
  const found = await vi.waitFor(() => {
    const list = [...document.body.querySelectorAll('[role="listbox"]')].pop()
    const match = [...(list?.querySelectorAll('[role="option"]') ?? [])].find(o => o.textContent?.trim() === option)
    expect(match).toBeDefined()
    return match as HTMLElement
  })
  found.dispatchEvent(new Event('pointerup', { bubbles: true }))
  await flushPromises()
}

describe('Departments page', () => {
  afterEach(() => {
    for (const component of mounted) component.unmount()
    mounted = []
  })

  beforeEach(async () => {
    document.body.innerHTML = ''
    await clearNuxtData()
    Object.assign(state, { identities: all(), calls: [], refuse: undefined })
  })

  it('shows an administrator every department with its members and leads, and the root apart', async () => {
    const component = await mount('administrator')
    await vi.waitFor(() => expect(component.findAll('[data-testid="department"]').length).toBe(2))
    const sales = component.get(`[data-department="${SALES}"]`)
    expect(sales.findAll('[data-member]').map(m => m.attributes('data-member'))).toEqual([PAM, PETER])
    expect(sales.get(`[data-member="${PAM}"] [data-testid="department-lead"]`).text()).toBe('Lead')
    expect(component.get('[data-testid="admin-only-identities"]').text()).toContain('Company root')
    // Only an administrator appoints a lead.
    expect(sales.find(`[data-member="${PETER}"] [data-testid="department-appoint"]`).exists()).toBe(true)
  })

  it('appoints a lead, and opens the root to departments', async () => {
    const component = await mount('administrator')
    await vi.waitFor(() => expect(component.find(`[data-department="${SUPPORT}"]`).exists()).toBe(true))
    await component.get(`[data-department="${SUPPORT}"] [data-member="${JAMES}"] [data-testid="department-appoint"]`).trigger('click')
    await vi.waitFor(() => expect(state.calls).toContainEqual({ method: 'PUT', path: `${SUPPORT}/${JAMES}`, body: { role: 'lead' } }))
    await component.get(`[data-admin-only="${ROOT}"] [data-testid="admin-only-open"]`).trigger('click')
    await vi.waitFor(() => expect(state.calls).toContainEqual({ method: 'PATCH', path: ROOT, body: { admin_only: false } }))
  })

  it('shows a lead only the departments they lead, and no lead controls over others', async () => {
    state.identities = [all()[0]!, { pubkey: SUPPORT, npub: 'npub1support', label: 'Support', is_primary: false, available: true, my_role: 'member' }]
    const component = await mount('signer')
    await vi.waitFor(() => expect(component.findAll('[data-testid="department"]').length).toBe(1))
    expect(component.find(`[data-department="${SUPPORT}"]`).exists()).toBe(false)
    expect(component.find('[data-testid="admin-only-identities"]').exists()).toBe(false)
    expect(component.find('[data-testid="department-appoint"]').exists()).toBe(false)
    expect(component.get('[data-testid="department-step-down"]').text()).toBe('Step down as lead')
  })

  it('lets a lead add an existing member as a member, and remove one', async () => {
    state.identities = [all()[0]!]
    vi.stubGlobal('confirm', () => true)
    const component = await mount('signer')
    await vi.waitFor(() => expect(component.find(`[data-department="${SALES}"]`).exists()).toBe(true))
    await pick(component, 'Add a team member to Sales', 'James')
    await component.get('[data-testid="department-add"]').trigger('click')
    await vi.waitFor(() => expect(state.calls).toContainEqual({ method: 'PUT', path: `${SALES}/${JAMES}`, body: { role: 'member' } }))
    await component.get(`[data-member="${PETER}"] [data-testid="department-remove"]`).trigger('click')
    await vi.waitFor(() => expect(state.calls).toContainEqual({ method: 'DELETE', path: `${SALES}/${PETER}` }))
    vi.unstubAllGlobals()
  })

  it('registers someone new into the department as a signer, and explains a refusal', async () => {
    state.identities = [all()[0]!]
    const component = await mount('signer')
    await vi.waitFor(() => expect(component.find(`[data-department="${SALES}"]`).exists()).toBe(true))
    await component.get('input[aria-label="Name of the new member of Sales"]').setValue('Paul')
    await component.get('input[aria-label="Pubkey of the new member of Sales"]').setValue('npub1paul')
    await component.get('[data-testid="department-register"]').trigger('click')
    await vi.waitFor(() => expect(state.calls).toContainEqual({ method: 'POST', path: 'team', body: { name: 'Paul', pubkey: 'npub1paul', role: 'signer', identities: [SALES] } }))

    state.refuse = 'not_your_department'
    await component.get('input[aria-label="Name of the new member of Sales"]').setValue('Paula')
    await component.get('input[aria-label="Pubkey of the new member of Sales"]').setValue('npub1paula')
    await component.get('[data-testid="department-register"]').trigger('click')
    await vi.waitFor(() => expect(component.get('[data-testid="department-error"]').text()).toBe('A lead registers signers only, into departments they lead.'))
  })

  it('says everyone may sign as the primary while grants don\'t matter yet', async () => {
    state.identities = [{ pubkey: 'e'.repeat(64), npub: 'npub1e', label: 'Primary', is_primary: true, available: true, members: [], leads: [] }]
    const component = await mount('administrator')
    await vi.waitFor(() => expect(component.find('[data-testid="department-everyone"]').exists()).toBe(true))
    expect(component.find('[data-testid="department-mark-admin-only"]').exists()).toBe(false)
  })
})
