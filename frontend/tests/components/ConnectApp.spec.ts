// Connecting an app from its own nostrconnect:// code (#148): the console reads the code through
// the bunker, shows what the app says about itself (unverified, and never its image) and what it
// asked for, pre-fills the form with that, and sends only what the member chooses.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import type { BunkerIdentity, Nip46Connection, NostrConnectPreview, NostrConnectRequest, TeamMember } from '#shared/types/bunker'
import ConnectApp from '~/components/connections/ConnectApp.vue'

const me = 'a'.repeat(64)
const bob = 'b'.repeat(64)
const APP_KEY = 'c'.repeat(64)
const CODE = `nostrconnect://${APP_KEY}?relay=wss%3A%2F%2Fapp-relay.example&secret=s3cret&perms=sign_event%3A1&image=https%3A%2F%2Ftracker.example%2Flogo.png`

const members: TeamMember[] = [
  { id: '1', name: 'Gary', pubkey: me, npub: 'npub1me', role: 'administrator' } as TeamMember,
  { id: '2', name: 'Bob', pubkey: bob, npub: 'npub1bob', role: 'signer' } as TeamMember
]
const identities: BunkerIdentity[] = [
  { pubkey: 'e'.repeat(64), npub: 'npub1primary', label: 'Primary', is_primary: true, available: true, members: [me, bob] },
  { pubkey: '9'.repeat(64), npub: 'npub1sales', label: 'Sales', is_primary: false, available: true, members: [bob] }
]

const preview = (over: Partial<NostrConnectPreview> = {}): NostrConnectPreview => ({
  client_pubkey: APP_KEY,
  name: 'Coracle',
  url: 'https://coracle.example',
  metadata_verified: false,
  relays: [{ url: 'wss://app-relay.example', bunker: false }],
  requested_kinds: [1],
  requested_any_kind: false,
  requested_methods: ['nip44_encrypt'],
  already_connected: false,
  ...over
})

let mounted: { unmount: () => void }[] = []

async function mount(props: { administrator?: boolean, preview?: (uri: string) => Promise<NostrConnectPreview>, connect?: (request: NostrConnectRequest) => Promise<Nip46Connection> }) {
  const InApp = defineComponent({
    render: () => h(UApp, () => h(ConnectApp, {
      members,
      self: props.administrator ? me : bob,
      administrator: props.administrator ?? false,
      identities,
      preview: props.preview ?? (async () => preview()),
      connect: props.connect ?? (async () => ({}) as Nip46Connection)
    }))
  })
  const component = await mountSuspended(InApp, { attachTo: document.body })
  mounted.push(component)
  return component
}

async function read(component: Awaited<ReturnType<typeof mount>>, code = CODE) {
  await component.find('#connect-app-code').setValue(code)
  await component.find('[data-testid="connect-app-read"]').trigger('submit')
  await flushPromises()
}

/** As `$fetch` rejects with the bunker's refusal under the proxy's error data. */
function refused(error: string, message: string) {
  return Object.assign(new Error('Bad Request'), { data: { data: { error, message } } })
}

describe('Connect an app (#148)', () => {
  afterEach(() => {
    for (const component of mounted) component.unmount()
    mounted = []
    document.body.innerHTML = ''
  })

  it('shows what the code says as unverified, never loads its image, and pre-fills what it asked for', async () => {
    const seen = vi.fn(async () => preview())
    const component = await mount({ preview: seen })
    await read(component)
    expect(seen).toHaveBeenCalledWith(CODE)
    const shown = component.find('[data-testid="connect-app-preview"]')
    expect(shown.text()).toContain('Coracle')
    expect(shown.text()).toContain('Says it is https://coracle.example')
    expect(shown.find('[data-testid="connect-app-unverified"]').exists()).toBe(true)
    expect(component.find('[data-testid="connect-app-asked"]').text()).toContain('only what it asked for can be granted')
    expect(component.find('[data-testid="connect-app-relays"]').text()).toContain('wss://app-relay.example')
    expect(component.find('[data-testid="connect-app"]').findAll('img')).toHaveLength(0)
    expect(component.html()).not.toContain('tracker.example/logo.png')
    expect((component.find('#connect-app-kinds').element as HTMLInputElement).value).toBe('1')
    expect((component.find('[data-method="nip44_encrypt"]').element as HTMLElement).getAttribute('data-state')).toBe('checked')
  })

  it('connects what a signer chose, for themselves, without approval, and says so', async () => {
    const connect = vi.fn(async (_: NostrConnectRequest) => ({}) as Nip46Connection)
    const component = await mount({ connect })
    await read(component)
    expect(component.find('#connect-app-for').exists()).toBe(false)
    expect(component.find('[data-testid="connect-app-approval"]').exists()).toBe(false)
    await component.find('#connect-app-kinds').setValue('1, 7')
    expect(component.find('[data-testid="connect-app-unasked"]').text()).toContain('did not ask for 7')
    await component.find('[data-testid="connect-app-confirm"]').trigger('submit')
    await flushPromises()
    expect(connect).toHaveBeenCalledTimes(1)
    const sent = connect.mock.calls[0]![0]
    expect(sent).toMatchObject({ uri: CODE, for_pubkey: bob, kinds: [1, 7], methods: ['nip44_encrypt'] })
    expect(sent.approval_kinds).toBeUndefined()
    expect(component.find('[data-testid="connect-app-done"]').text()).toContain('Connected Coracle')
    expect(component.find('[data-testid="connect-app-confirm"]').exists()).toBe(false)
  })

  it('lets an administrator choose the member and kinds that need approval', async () => {
    const connect = vi.fn(async (_: NostrConnectRequest) => ({}) as Nip46Connection)
    const component = await mount({ administrator: true, connect })
    await read(component)
    expect(component.find('#connect-app-for').exists()).toBe(true)
    await component.find('#connect-app-approval-1').trigger('click')
    await component.find('[data-testid="connect-app-confirm"]').trigger('submit')
    await flushPromises()
    expect(connect.mock.calls[0]![0]).toMatchObject({ for_pubkey: me, kinds: [1], approval_kinds: [1] })
  })

  it('shows the bunker\'s reason for a code it refuses, and nothing to confirm', async () => {
    const component = await mount({ preview: async () => { throw refused('invalid_nostrconnect_uri', 'The code has no secret, which NIP-46 requires.') } })
    await read(component, `nostrconnect://${APP_KEY}?relay=wss%3A%2F%2Fr.example`)
    expect(component.find('[data-testid="connect-app-error"]').text()).toContain('The code has no secret')
    expect(component.find('[data-testid="connect-app-confirm"]').exists()).toBe(false)
  })

  it('will not connect an app that is already connected', async () => {
    const component = await mount({ preview: async () => preview({ already_connected: true }) })
    await read(component)
    expect(component.find('[data-testid="connect-app-already"]').exists()).toBe(true)
    const button = component.find('[data-testid="connect-app-confirm"] button[type="submit"]')
    expect(button.attributes('disabled')).toBeDefined()
  })

  it('says when no relay took the bunker\'s answer', async () => {
    const component = await mount({ connect: async () => { throw refused('connect_undelivered', 'x') } })
    await read(component)
    await component.find('[data-testid="connect-app-confirm"]').trigger('submit')
    await flushPromises()
    expect(component.find('[data-testid="connect-app-error"]').text()).toContain('None of the app\'s relays took the bunker\'s answer')
  })
})
