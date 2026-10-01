import { describe, it, expect } from 'vitest'
import { checkImage, MAX_GIF_BYTES, MAX_STILL_BYTES, scaledSize } from '~/utils/image'

describe('profile images', () => {
  it('accepts the supported types within their limits', () => {
    for (const type of ['image/png', 'image/jpeg', 'image/webp']) expect(checkImage({ type, size: MAX_STILL_BYTES })).toBeUndefined()
    expect(checkImage({ type: 'image/gif', size: MAX_GIF_BYTES })).toBeUndefined()
  })

  it('refuses other types and oversized files', () => {
    expect(checkImage({ type: 'image/svg+xml', size: 10 })).toContain('PNG, JPEG, WebP or GIF')
    expect(checkImage({ type: 'image/png', size: MAX_STILL_BYTES + 1 })).toContain('10 MB')
    expect(checkImage({ type: 'image/gif', size: MAX_GIF_BYTES + 1 })).toContain('5 MB')
  })

  it('scales the longest side down, never up', () => {
    expect(scaledSize(2000, 1000, 512)).toEqual({ width: 512, height: 256 })
    expect(scaledSize(300, 600, 512)).toEqual({ width: 256, height: 512 })
    expect(scaledSize(100, 50, 512)).toEqual({ width: 100, height: 50 })
  })
})
