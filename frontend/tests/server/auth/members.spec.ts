// @vitest-environment node
import { describe, it, expect } from 'vitest'
import { lacksAdministrator } from '../../../server/utils/auth/members'
import type { HealthCheck } from '#shared/types/bunker'

const check = (name: string, status: HealthCheck['status']): HealthCheck => ({ name, status, detail: '' })

describe('lacksAdministrator (#74)', () => {
  it('is true only when the administrator check warns', () => {
    expect(lacksAdministrator({ checks: [check('signer', 'pass'), check('administrator', 'warn')] })).toBe(true)
    expect(lacksAdministrator({ checks: [check('administrator', 'pass')] })).toBe(false)
  })

  it('does not read a failed team read, or another check warning, as no administrator', () => {
    expect(lacksAdministrator({ checks: [check('administrator', 'fail')] })).toBe(false)
    expect(lacksAdministrator({ checks: [check('relays', 'warn'), check('administrator', 'pass')] })).toBe(false)
  })

  it('is false for a bunker that predates the check', () => {
    expect(lacksAdministrator({ checks: [check('signer', 'pass'), check('database', 'pass'), check('relays', 'disabled')] })).toBe(false)
  })
})
