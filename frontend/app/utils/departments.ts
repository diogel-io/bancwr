// Departments (#147, departments ADR): each identity that isn't administrator only is one
// department, its members are the members granted it, and a lead also manages it.
import type { BunkerIdentity, DepartmentRole, TeamMember } from '#shared/types/bunker'

/** The identities listed as departments: not administrator only, and listed with their members. */
export function departmentsOf(identities: readonly BunkerIdentity[]): BunkerIdentity[] {
  return identities.filter(identity => !identity.admin_only && identity.members !== undefined)
}

/** A member's role in a department, or undefined when they are not in it. */
export function roleIn(identity: BunkerIdentity, member: string): DepartmentRole | undefined {
  if ((identity.leads ?? []).includes(member)) return 'lead'
  return (identity.members ?? []).includes(member) ? 'member' : undefined
}

/** "Lead of Sales and Support" for a member who leads departments, or undefined. */
export function leadOf(identities: readonly BunkerIdentity[], member: string): string | undefined {
  const labels = identities.filter(identity => (identity.leads ?? []).includes(member)).map(identity => identity.label)
  if (!labels.length) return undefined
  const last = labels.pop()!
  return `Lead of ${labels.length ? `${labels.join(', ')} and ${last}` : last}`
}

/** The team members of a department, with their roles, leads first, then by name. */
export function membersOf(identity: BunkerIdentity, team: readonly TeamMember[]): { member: TeamMember, role: DepartmentRole }[] {
  return team
    .flatMap((member) => {
      const role = roleIn(identity, member.pubkey)
      return role ? [{ member, role }] : []
    })
    .sort((a, b) => (a.role === b.role ? a.member.name.localeCompare(b.member.name) : a.role === 'lead' ? -1 : 1))
}

/** What the bunker's department refusals mean, in the console's words. */
export const DEPARTMENT_ERRORS: Readonly<Record<string, string>> = {
  forbidden: 'Only an administrator or a lead of this department can do that.',
  not_your_department: 'A lead registers signers only, into departments they lead.',
  admin_only: 'This identity is administrator only: it has no members.',
  has_members: 'Members may still sign as this identity. Remove them from it first.',
  invalid_role: 'A department role is member or lead.',
  not_registered: 'That key is not a member of the vault.'
}
