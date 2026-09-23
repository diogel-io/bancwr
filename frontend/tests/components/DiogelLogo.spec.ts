import { describe, it, expect } from 'vitest'
import { mountSuspended } from '@nuxt/test-utils/runtime'
import DiogelLogo from '~/components/DiogelLogo.vue'

// The build may inline an SVG as a data URI or emit it as a URL, so identify a mark by its own
// artwork rather than by a filename. The two marks differ in their non-brand fills: the light one
// is drawn in #111827, the dark one in #ffffff.
function markOf(src: string): 'light' | 'dark' | 'unknown' {
  const svg = decodeURIComponent(src)
  if (svg.includes('#ffffff') || src.includes('/dark/')) return 'dark'
  if (svg.includes('#111827') || src.includes('/light/')) return 'light'
  return 'unknown'
}

describe('DiogelLogo', () => {
  it('renders both marks so CSS can pick one without a hydration mismatch', async () => {
    const component = await mountSuspended(DiogelLogo)
    const images = component.findAll('img')

    expect(images).toHaveLength(2)

    const light = images.find(img => img.classes().includes('dark:hidden'))
    const dark = images.find(img => img.classes().includes('dark:block'))

    expect(markOf(light!.attributes('src')!)).toBe('light')
    expect(markOf(dark!.attributes('src')!)).toBe('dark')
  })

  it('carries the Diogel primary in both marks', async () => {
    const component = await mountSuspended(DiogelLogo)

    for (const img of component.findAll('img')) {
      expect(decodeURIComponent(img.attributes('src')!)).toContain('#f97316')
    }
  })

  it('is labelled once for assistive technology', async () => {
    const component = await mountSuspended(DiogelLogo)
    const images = component.findAll('img')

    // Only the visible-in-light mark carries the label; the other is decorative, so a screen
    // reader announces "Diogel" once rather than twice.
    expect(images.map(img => img.attributes('alt'))).toEqual(['Diogel', ''])
    expect(images[1]!.attributes('aria-hidden')).toBe('true')
  })

  it('applies the requested size', async () => {
    const component = await mountSuspended(DiogelLogo, { props: { size: 'lg' } })

    expect(component.get('span').classes()).toContain('w-[90px]')
  })
})
