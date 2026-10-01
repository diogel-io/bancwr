// The kind 0 profile (#30): parsing it, choosing the newest, and merging the form into it.
// Pure, so the rule that matters most (never drop a field another app set) is tested directly.
import type { NostrEvent } from 'nostr-tools/pure'

/** A kind 0 content object: whatever keys any client has written, not only the ones edited here. */
export type ProfileContent = Record<string, unknown>

/** The text fields the form edits, Porwr's set (ProfileEditor.vue, profile-service.ts). */
export const TEXT_FIELDS = ['name', 'display_name', 'about', 'website', 'picture', 'banner', 'nip05', 'lud16'] as const
export type TextField = typeof TEXT_FIELDS[number]

/** The form's values. Birthday parts are strings because they come from inputs. */
export interface ProfileForm extends Record<TextField, string> {
  bot: boolean
  birthday: { year: string, month: string, day: string }
}

export function emptyForm(): ProfileForm {
  return {
    name: '', display_name: '', about: '', website: '', picture: '', banner: '', nip05: '', lud16: '',
    bot: false,
    birthday: { year: '', month: '', day: '' }
  }
}

/** The content as an object, or null when it is not a JSON object. */
export function parseProfileContent(content: string): ProfileContent | null {
  try {
    const value: unknown = JSON.parse(content)
    return typeof value === 'object' && value !== null && !Array.isArray(value) ? value as ProfileContent : null
  } catch {
    return null
  }
}

/** The replaceable event that wins (NIP-01): the newest, and on a tie the lowest id. */
export function newestEvent<T extends Pick<NostrEvent, 'created_at' | 'id'>>(events: T[]): T | undefined {
  return events.reduce<T | undefined>((best, event) => {
    if (!best || event.created_at > best.created_at) return event
    if (event.created_at === best.created_at && event.id < best.id) return event
    return best
  }, undefined)
}

export function formFromContent(content: ProfileContent | null): ProfileForm {
  const form = emptyForm()
  if (!content) return form
  for (const field of TEXT_FIELDS) {
    const value = content[field]
    if (typeof value === 'string') form[field] = value
  }
  form.bot = content.bot === true
  const birthday = content.birthday
  if (typeof birthday === 'object' && birthday !== null) {
    for (const part of ['year', 'month', 'day'] as const) {
      const value = (birthday as Record<string, unknown>)[part]
      if (typeof value === 'number' && Number.isInteger(value)) form.birthday[part] = String(value)
    }
  }
  return form
}

/** `about` keeps its whitespace; the single-line fields are trimmed. */
function cleaned(field: TextField, value: string): string {
  return field === 'about' ? value.replace(/\s+$/u, '') : value.trim()
}

/**
 * The content to publish: the latest content as it stands, with only the form's fields changed.
 * Every key the form does not edit is kept as it was, so fields set in another client survive
 * (#30). A form field left empty is removed, as is `bot` when false and a birthday with no parts.
 */
export function mergeProfile(latest: ProfileContent | null, form: ProfileForm): ProfileContent {
  const edited: ProfileContent = {}
  for (const field of TEXT_FIELDS) {
    const value = cleaned(field, form[field])
    if (value) edited[field] = value
  }
  if (form.bot) edited.bot = true

  const birthday: Record<string, number> = {}
  for (const part of ['year', 'month', 'day'] as const) {
    const value = form.birthday[part].trim()
    if (value) birthday[part] = Number(value)
  }
  if (Object.keys(birthday).length) edited.birthday = birthday

  // Every key the form owns is replaced by the form's value, or dropped when the form has none.
  const owned = new Set<string>([...TEXT_FIELDS, 'bot', 'birthday'])
  const kept = Object.fromEntries(Object.entries(latest ?? {}).filter(([key]) => !owned.has(key)))
  return { ...kept, ...edited }
}

/** Problems that stop a save, by field. Empty when the form can be saved. */
export function validateForm(form: ProfileForm): Partial<Record<TextField | 'birthday', string>> {
  const errors: Partial<Record<TextField | 'birthday', string>> = {}
  for (const field of ['website', 'picture', 'banner'] as const) {
    const value = form[field].trim()
    if (value && !isHttpUrl(value)) errors[field] = 'Enter a full address starting with https://'
  }
  const lud16 = form.lud16.trim()
  if (lud16 && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/u.test(lud16)) errors.lud16 = 'A Lightning address looks like name@domain.com'

  const { year, month, day } = form.birthday
  const inRange = (value: string, min: number, max: number) => !value.trim() || (/^\d+$/u.test(value.trim()) && Number(value) >= min && Number(value) <= max)
  if (!inRange(year, 1, 9999) || !inRange(month, 1, 12) || !inRange(day, 1, 31)) {
    errors.birthday = 'Use a year, a month from 1 to 12 and a day from 1 to 31, or leave them empty.'
  }
  return errors
}

export function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:'
  } catch {
    return false
  }
}
