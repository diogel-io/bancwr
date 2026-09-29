import { describe, it, expect } from 'vitest'
import { versionOrDefault, DEFAULT_VERSION } from '../../server/utils/version'

describe('versionOrDefault', () => {
  it('reports the version the image was built with', () => {
    expect(versionOrDefault('0.1.0-49')).toBe('0.1.0-49')
  })

  it('trims surrounding whitespace', () => {
    expect(versionOrDefault(' 0.1.0\n')).toBe('0.1.0')
  })

  it('falls back to 0.0.0 when unset or blank', () => {
    expect(versionOrDefault(undefined)).toBe(DEFAULT_VERSION)
    expect(versionOrDefault('  ')).toBe(DEFAULT_VERSION)
    expect(DEFAULT_VERSION).toBe('0.0.0')
  })
})
