import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import { flushPromises, type DOMWrapper } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { UApp } from '#components'
import ProfilePreview from '~/components/profile/ProfilePreview.vue'
import ImageField from '~/components/profile/ImageField.vue'
import FollowRow from '~/components/follows/FollowRow.vue'
import AddFollow from '~/components/follows/AddFollow.vue'
import ConnectionRow from '~/components/connections/ConnectionRow.vue'
import { emptyForm } from '~/utils/profile'
import type { Nip46Connection } from '#shared/types/bunker'

// Each third-party picture loads with no referrer, the policy set before src (#75): the browser
// starts the request when src is set, so a policy applied after it comes too late (#72).
function expectNoReferrer(img: DOMWrapper<Element>, src: string) {
  expect(img.attributes('src')).toBe(src)
  expect(img.attributes('referrerpolicy')).toBe('no-referrer')
  const order = (img.element as HTMLImageElement).getAttributeNames()
  expect(order.indexOf('referrerpolicy')).toBeLessThan(order.indexOf('src'))
}

const key = 'b'.repeat(64)

describe('third-party images send no Referer (#75)', () => {
  it('the profile preview: banner and picture', async () => {
    const form = { ...emptyForm(), name: 'alice', banner: 'https://img.example/banner.png', picture: 'https://img.example/me.png' }
    const component = await mountSuspended(ProfilePreview, { props: { form, npub: 'npub1x' } })
    const images = component.findAll('img')
    expectNoReferrer(images.find(img => img.attributes('src')?.endsWith('banner.png'))!, 'https://img.example/banner.png')
    expectNoReferrer(images.find(img => img.attributes('src')?.endsWith('me.png'))!, 'https://img.example/me.png')
  })

  it('an image field\'s preview, which still falls back when the image breaks', async () => {
    const component = await mountSuspended(ImageField, {
      props: { label: 'Picture', kind: 'picture', modelValue: 'https://img.example/me.png', upload: async () => '' }
    })
    const img = component.find('img')
    expectNoReferrer(img, 'https://img.example/me.png')
    await img.trigger('error')
    expect(component.find('img').exists()).toBe(false)
  })

  it('a followed key\'s avatar', async () => {
    const component = await mountSuspended(FollowRow, {
      props: { follow: { pubkey: key, tag: ['p', key] }, profile: { name: 'Bob', picture: 'https://img.example/bob.png', createdAt: 1 } }
    })
    expectNoReferrer(component.find('img'), 'https://img.example/bob.png')
  })

  it('the avatar previewed before following someone', async () => {
    const component = await mountSuspended(AddFollow, {
      props: {
        following: new Set<string>(),
        self: 'a'.repeat(64),
        profiles: { [key]: { name: 'Bob', picture: 'https://img.example/bob.png', createdAt: 1 } },
        loadProfiles: async () => {}
      }
    })
    await component.find('input').setValue(key)
    await component.find('form').trigger('submit')
    await flushPromises()
    expectNoReferrer(component.find('[data-testid="add-follow-preview"] img'), 'https://img.example/bob.png')
  })

  it('a connected app\'s icon, which the app itself names', async () => {
    const connection: Nip46Connection = {
      id: 'c1', client_pubkey: key, for_pubkey: 'a'.repeat(64), client_name: 'App', client_url: null,
      client_image: 'https://app.example/icon.png', metadata_verified: false, kinds: [1],
      connected_at: '2026-10-01T00:00:00Z', last_used_at: null, revoked_at: null, revoked_reason: null, revoked_by: null
    }
    // It has tooltips, which need the provider UApp installs in app.vue.
    const Row = defineComponent({ render: () => h(UApp, () => h(ConnectionRow, { connection })) })
    const component = await mountSuspended(Row)
    expectNoReferrer(component.find('img'), 'https://app.example/icon.png')
  })
})
