import { describe, it, expect } from 'vitest'

// Deliberately failing: negative check for the test gate in #47. Never merged.
describe('negative check', () => {
  it('must fail', () => {
    expect(1 + 1).toBe(3)
  })
})
