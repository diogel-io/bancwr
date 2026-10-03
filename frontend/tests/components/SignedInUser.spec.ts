import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockNuxtImport, mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h, ref } from 'vue'
import { useState } from '#imports'
import { UApp } from '#components'
import SignedInUser from '~/components/SignedInUser.vue'

const npub = 'npub1wsmmysc6a60nk8dswq706zcvu7auzyz0869z9dryze5c8shd5z8qnd57dv'
const { signOut } = vi.hoisted(() => ({ signOut: vi.fn() }))
mockNuxtImport('useAuth', () => () => ({
  state: ref({ status: 'signed-in', pubkey: 'a'.repeat(64), npub, role: 'administrator' }),
  signOut
}))

// No relays from tests: the key's profile is whatever the test says (#72).
const { found } = vi.hoisted(() => ({ found: { value: undefined as unknown } }))
vi.mock('~/utils/key-profile', () => ({
  lookupKeyProfile: async () => found.value,
  lookupRelays: () => [],
  queryRelays: async () => []
}))

async function mountFooter(collapsed = false) {
  // Collapsed, it renders tooltips, which need the provider UApp installs in app.vue.
  const Footer = defineComponent({ render: () => h(UApp, () => h(SignedInUser, { collapsed })) })
  const component = await mountSuspended(Footer)
  await flushPromises()
  return component
}

describe('SignedInUser', () => {
  beforeEach(() => {
    found.value = undefined
    useState('signed-in-profile').value = { pubkey: '', status: 'idle' }
  })

  it('shows the signed-in key and role, and signs out', async () => {
    const component = await mountFooter()
    expect(component.find('[data-testid="user-name"]').text()).toBe(`${npub.slice(0, 12)}…${npub.slice(-4)}`)
    expect(component.text()).toContain('Administrator')
    expect(component.find('[role="group"]').attributes('aria-label')).toBe(`Signed in as ${npub}, Administrator`)
    await component.find('button[aria-label="Sign out"]').trigger('click')
    expect(signOut).toHaveBeenCalled()
  })

  it('shows the profile picture and name, keeping the npub in the title and accessible name (#72)', async () => {
    found.value = { name: 'Alice', picture: 'https://img.example/alice.png', createdAt: 1 }
    const component = await mountFooter()

    const img = component.find('[data-testid="user-avatar"] img, img[data-testid="user-avatar"]')
    expect(img.attributes('src')).toBe('https://img.example/alice.png')
    expect(img.attributes('referrerpolicy')).toBe('no-referrer')
    // Set before src: the browser starts the request when src is set, and an attribute applied
    // after it came too late, so the e2e test saw the Referer sent.
    const order = (img.element as HTMLImageElement).getAttributeNames()
    expect(order.indexOf('referrerpolicy')).toBeLessThan(order.indexOf('src'))
    const name = component.find('[data-testid="user-name"]')
    expect(name.text()).toBe('Alice')
    expect(name.attributes('title')).toBe(npub)
    expect(component.find('[role="group"]').attributes('aria-label')).toBe(`Signed in as ${npub}, Administrator`)
  })

  it('shows the user icon, and no image, without a picture (#72)', async () => {
    found.value = { name: 'Alice', createdAt: 1 }
    const component = await mountFooter()
    expect(component.find('img').exists()).toBe(false)
    expect(component.html()).toContain('i-lucide:user')
  })

  it('shows the user icon when the picture fails to load (#72)', async () => {
    found.value = { name: 'Alice', picture: 'https://img.example/broken.png', createdAt: 1 }
    const component = await mountFooter()
    await component.find('img').trigger('error')
    expect(component.find('img').exists()).toBe(false)
    expect(component.html()).toContain('i-lucide:user')
  })

  it('shows the user icon while the profile is still loading (#72)', async () => {
    found.value = new Promise(() => {})
    const component = await mountFooter()
    expect(component.find('img').exists()).toBe(false)
    expect(component.html()).toContain('i-lucide:user')
  })

  it('collapsed: shows the avatar on its own, and still signs out with the identity in its name', async () => {
    found.value = { name: 'Alice', picture: 'https://img.example/alice.png', createdAt: 1 }
    const component = await mountFooter(true)
    expect(component.text()).not.toContain('Administrator')
    expect(component.find('img').attributes('src')).toBe('https://img.example/alice.png')
    const signOutButton = component.find('button')
    expect(signOutButton.attributes('aria-label')).toBe(`Sign out. Signed in as ${npub}, Administrator`)
    await signOutButton.trigger('click')
    expect(signOut).toHaveBeenCalled()
  })
})
