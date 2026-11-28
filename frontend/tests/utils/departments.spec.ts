import { describe, it, expect } from 'vitest'
import type { BunkerIdentity, TeamMember } from '#shared/types/bunker'
import { departmentsOf, leadOf, membersOf, roleIn } from '~/utils/departments'

// Departments (#147): an identity, its members and its leads, as the bunker lists them.
const PAM = 'a'.repeat(64)
const PETER = 'b'.repeat(64)
const JAMES = 'c'.repeat(64)
const identity = (label: string, members?: string[], leads: string[] = [], extra: Partial<BunkerIdentity> = {}): BunkerIdentity =>
  ({ pubkey: label.toLowerCase().padEnd(64, '0'), npub: null, label, is_primary: false, available: true, members, leads, ...extra })
const member = (name: string, pubkey: string): TeamMember => ({ id: name, name, pubkey, npub: `npub1${name}`, role: 'signer' })

describe('departments', () => {
  const sales = identity('Sales', [PAM, PETER], [PAM])
  const support = identity('Support', [JAMES, PAM], [JAMES, PAM])

  it('lists as departments only identities with their members that are not administrator only', () => {
    const root = identity('Root', [], [], { admin_only: true })
    const usedOnly = identity('Marketing')
    expect(departmentsOf([sales, root, usedOnly, support]).map(i => i.label)).toEqual(['Sales', 'Support'])
  })

  it('reads a member\'s role in a department', () => {
    expect(roleIn(sales, PAM)).toBe('lead')
    expect(roleIn(sales, PETER)).toBe('member')
    expect(roleIn(sales, JAMES)).toBeUndefined()
  })

  it('names the departments a member leads, as "Lead of"', () => {
    expect(leadOf([sales, support], PAM)).toBe('Lead of Sales and Support')
    expect(leadOf([sales, support], JAMES)).toBe('Lead of Support')
    expect(leadOf([sales, support], PETER)).toBeUndefined()
    expect(leadOf([sales, support, identity('Engineering', [PAM], [PAM])], PAM)).toBe('Lead of Sales, Support and Engineering')
  })

  it('lists a department\'s members, leads first, then by name', () => {
    const team = [member('Peter', PETER), member('James', JAMES), member('Pam', PAM)]
    expect(membersOf(sales, team).map(m => `${m.member.name}:${m.role}`)).toEqual(['Pam:lead', 'Peter:member'])
  })
})
