import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { defineComponent, h, nextTick } from 'vue'
import { useNip07 } from '~/composables/useNip07'

type Nip07 = ReturnType<typeof useNip07>

function mountWithNip07() {
  let nip07!: Nip07
  const component = mount(defineComponent({
    setup() {
      nip07 = useNip07()
      return () => h('div')
    }
  }))
  return { component, nip07 }
}

const win = window as unknown as { nostr?: unknown }

describe('useNip07', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    delete win.nostr
  })
  afterEach(() => {
    delete win.nostr
    vi.useRealTimers()
  })

  it('reports no extension after about a second of polling', async () => {
    const { nip07 } = mountWithNip07()
    expect(nip07.available.value).toBeUndefined()
    await vi.advanceTimersByTimeAsync(1000)
    expect(nip07.available.value).toBe(false)
  })

  it('finds an extension that is injected just after mount', async () => {
    const { nip07 } = mountWithNip07()
    await vi.advanceTimersByTimeAsync(300)
    win.nostr = { getPublicKey: vi.fn(), signEvent: vi.fn() }
    await vi.advanceTimersByTimeAsync(100)
    expect(nip07.available.value).toBe(true)
  })

  it('stops polling when unmounted, leaving no timer behind', async () => {
    const { component, nip07 } = mountWithNip07()
    await vi.advanceTimersByTimeAsync(200)
    component.unmount()
    await nextTick()
    expect(vi.getTimerCount()).toBe(0)

    win.nostr = { getPublicKey: vi.fn(), signEvent: vi.fn() }
    await expect(vi.advanceTimersByTimeAsync(2000)).resolves.not.toThrow()
    expect(nip07.available.value).toBeUndefined()
  })
})
