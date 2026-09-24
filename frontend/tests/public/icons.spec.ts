import { describe, it, expect } from 'vitest'
import { readFileSync, existsSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(__dirname, '../..')
const icons = ['favicon.svg', 'favicon.ico', 'apple-touch-icon.png']

describe('site icons', () => {
  it.each(icons)('ships public/%s', (name) => {
    expect(existsSync(resolve(root, 'public', name))).toBe(true)
  })

  it.each(icons)('links /%s from nuxt.config', (name) => {
    const config = readFileSync(resolve(root, 'nuxt.config.ts'), 'utf8')

    expect(config).toContain(`/${name}`)
  })

  it('serves the Diogel mark rather than a default icon', () => {
    // The .ico shipped before this was Nuxt's own. Checking for the brand primary is what
    // distinguishes "our mark" from "whatever the scaffold left behind"; a bare existence check
    // would have passed on the Nuxt icon too.
    const ico = readFileSync(resolve(root, 'public/favicon.ico'))
    const svg = readFileSync(resolve(root, 'public/favicon.svg'), 'utf8')

    expect(svg).toContain('#f97316')
    // 0xf9 0x73 0x16 as BGR, which is how the colour appears in the .ico bitmap data.
    expect(ico.includes(Buffer.from([0x16, 0x73, 0xf9]))).toBe(true)
  })

  it('follows the browser colour scheme in the SVG', () => {
    const svg = readFileSync(resolve(root, 'public/favicon.svg'), 'utf8')

    // The light mark's glyph sinks into dark browser chrome, so the SVG has to swap it.
    expect(svg).toContain('prefers-color-scheme: dark')
    expect(svg).toContain('#ffffff')
    expect(svg).toContain('#111827')
  })
})
