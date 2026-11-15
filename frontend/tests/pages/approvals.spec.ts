// The approval queue (#102): what an administrator sees of a request waiting (the event as plain
// text, never fetched or rendered as HTML), approving with the event id and a note, rejecting, the
// bunker's refusals, and a signer's read-only view.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { readBody, setResponseStatus } from 'h3'
import { UApp } from '#components'
import type { PendingApproval, Role } from '#shared/types/bunker'
import { signInAs } from '../helpers/session'
import Approvals from '~/pages/approvals/index.vue'
import { clearNuxtData } from '#imports'

const me = 'a'.repeat(64)

const waiting = (over: Partial<PendingApproval> = {}): PendingApproval => ({
  id: 'p1', member_pubkey: 'b'.repeat(64), member_name: 'Bob', connection_id: 'c1', client_pubkey: 'c'.repeat(64),
  client_name: 'Scheduler', kind: 1, content: 'Launch <b>today</b> https://img.example/x.png', tags: [['t', 'launch']],
  created_at: 1_790_000_000, event_id: 'e'.repeat(64), requested_at: new Date(Date.now() - 60_000).toISOString(),
  expires_at: new Date(Date.now() + 29 * 60_000).toISOString(), ...over
})

const state = vi.hoisted(() => ({
  pending: [] as unknown[],
  decisions: [] as { path: string, body: unknown }[],
  refuse: undefined as string | undefined
}))

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))
registerEndpoint('/api/bunker/approvals', () => state.pending)
for (const decision of ['approve', 'reject']) {
  registerEndpoint(`/api/bunker/approvals/p1/${decision}`, {
    method: 'POST',
    handler: async (event) => {
      if (state.refuse) {
        setResponseStatus(event, state.refuse === 'not_found' ? 404 : 409)
        return { error: state.refuse, message: 'no' }
      }
      state.decisions.push({ path: decision, body: await readBody(event) })
      state.pending = []
      return { success: true }
    }
  })
}

const InApp = defineComponent({ render: () => h(UApp, () => h(Approvals)) })
let mounted: { unmount: () => void }[] = []

async function mount(role: Role) {
  signInAs(role)
  const component = await mountSuspended(InApp, { attachTo: document.body })
  mounted.push(component)
  await flushPromises()
  return component
}

describe('Approvals page', () => {
  afterEach(() => {
    for (const component of mounted) component.unmount()
    mounted = []
  })

  beforeEach(async () => {
    document.body.innerHTML = ''
    await clearNuxtData()
    Object.assign(state, { pending: [], decisions: [], refuse: undefined })
  })

  it('says when nothing is waiting', async () => {
    const component = await mount('administrator')
    expect(component.find('[data-testid="approvals-empty"]').text()).toBe('Nothing is waiting for approval.')
  })

  it('shows the event as plain text, with who asked and which app', async () => {
    state.pending = [waiting()]
    const component = await mount('administrator')
    const card = component.find('[data-approval="p1"]')
    expect(card.text()).toContain('Sign 1 Note')
    expect(card.text()).toContain('From Scheduler (unverified), connected for Bob')
    const content = card.find('[data-testid="approval-content"]')
    expect(content.text()).toBe('Launch <b>today</b> https://img.example/x.png')
    expect(content.find('b').exists()).toBe(false)
    expect(card.find('img').exists()).toBe(false)
    expect(card.find('a').exists()).toBe(false)
    expect(card.find('[data-testid="approval-tags"]').text()).toContain('["t","launch"]')
    expect(card.find('[data-testid="approval-event-id"]').text()).toBe('e'.repeat(64))
  })

  it('approves with the event id shown and the note', async () => {
    state.pending = [waiting()]
    const component = await mount('administrator')
    await component.find('input[maxlength="280"]').setValue('Ship it')
    await component.find('[data-testid="approval-approve"]').trigger('click')
    await vi.waitFor(() => expect(state.decisions).toHaveLength(1))
    expect(state.decisions[0]).toEqual({ path: 'approve', body: { event_id: 'e'.repeat(64), note: 'Ship it' } })
    await vi.waitFor(() => expect(component.find('[data-testid="approval-decided"]').text()).toContain('Approved'))
  })

  it('rejects, with no note when none was given', async () => {
    state.pending = [waiting()]
    const component = await mount('administrator')
    await component.find('[data-testid="approval-reject"]').trigger('click')
    await vi.waitFor(() => expect(state.decisions).toHaveLength(1))
    expect(state.decisions[0]).toEqual({ path: 'reject', body: {} })
  })

  it('explains why the bunker refused a decision', async () => {
    state.pending = [waiting()]
    state.refuse = 'connection_ended'
    const component = await mount('administrator')
    await component.find('[data-testid="approval-approve"]').trigger('click')
    await vi.waitFor(() => expect(component.find('[data-testid="approval-error"]').text()).toBe('The app\'s connection has ended, so nothing was signed.'))
  })

  it('shows a signer their own requests waiting, without the buttons', async () => {
    state.pending = [waiting({ member_pubkey: me, member_name: 'Gary' })]
    const component = await mount('signer')
    const card = component.find('[data-approval="p1"]')
    expect(card.text()).toContain('connected for you')
    expect(component.find('[data-testid="approval-approve"]').exists()).toBe(false)
    expect(component.find('[data-testid="approval-reject"]').exists()).toBe(false)
    expect(component.find('[data-testid="approvals-explainer"]').text()).toContain('An administrator must approve these')
  })
})
