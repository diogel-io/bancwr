import { describe, it, expect } from 'vitest'
import { emptyForm, formFromContent, mergeProfile, newestEvent, parseProfileContent, validateForm, type ProfileForm } from '~/utils/profile'

const full = (): ProfileForm => ({
  name: 'alice', display_name: 'Alice', about: 'Line one\nLine two', website: 'https://alice.example',
  picture: 'https://img.example/a.webp', banner: 'https://img.example/b.webp', nip05: 'alice@alice.example',
  lud16: 'alice@wallet.example', bot: true, birthday: { year: '1990', month: '4', day: '12' }
})

describe('profile content', () => {
  it('parses only JSON objects', () => {
    expect(parseProfileContent('{"name":"a"}')).toEqual({ name: 'a' })
    for (const bad of ['', 'not json', '[1]', 'null', '"text"']) expect(parseProfileContent(bad), bad).toBeNull()
  })

  it('picks the newest event, and the lowest id on a tie (NIP-01)', () => {
    expect(newestEvent([{ created_at: 1, id: 'b' }, { created_at: 3, id: 'c' }, { created_at: 2, id: 'a' }])?.id).toBe('c')
    expect(newestEvent([{ created_at: 5, id: 'f' }, { created_at: 5, id: 'e' }])?.id).toBe('e')
    expect(newestEvent([])).toBeUndefined()
  })

  it('reads every field into the form, ignoring values of the wrong type', () => {
    const form = formFromContent({
      name: 'alice', display_name: 'Alice', about: 'hi', website: 'https://a.example', picture: 'p', banner: 'b',
      nip05: 'a@a.example', lud16: 'a@w.example', bot: true, birthday: { year: 1990, month: 4, day: 12 }, extra: 'kept elsewhere'
    })
    expect(form).toEqual({
      name: 'alice', display_name: 'Alice', about: 'hi', website: 'https://a.example', picture: 'p', banner: 'b',
      nip05: 'a@a.example', lud16: 'a@w.example', bot: true, birthday: { year: '1990', month: '4', day: '12' }
    })
    expect(formFromContent({ name: 42, bot: 'yes', birthday: 'soon' })).toEqual(emptyForm())
  })
})

describe('mergeProfile', () => {
  it('keeps every field the form does not edit, as it was (#30)', () => {
    const latest = { name: 'old', zapper: { a: 1 }, pronouns: 'they/them', custom_flag: false, lud06: 'lnurl1…' }
    const merged = mergeProfile(latest, full())
    expect(merged).toMatchObject({ zapper: { a: 1 }, pronouns: 'they/them', custom_flag: false, lud06: 'lnurl1…' })
    expect(merged.name).toBe('alice')
  })

  it('writes every form field', () => {
    expect(mergeProfile(null, full())).toEqual({
      name: 'alice', display_name: 'Alice', about: 'Line one\nLine two', website: 'https://alice.example',
      picture: 'https://img.example/a.webp', banner: 'https://img.example/b.webp', nip05: 'alice@alice.example',
      lud16: 'alice@wallet.example', bot: true, birthday: { year: 1990, month: 4, day: 12 }
    })
  })

  it('removes a field the member emptied, bot when off, and a birthday with no parts', () => {
    const latest = { ...mergeProfile(null, full()), other: 1 }
    expect(mergeProfile(latest, emptyForm())).toEqual({ other: 1 })
  })

  it('keeps a partial birthday, trims single-line fields, and keeps about\'s inner whitespace', () => {
    const form = { ...emptyForm(), name: '  alice ', about: '  indented\n\n  text  \n', birthday: { year: '', month: '4', day: '' } }
    expect(mergeProfile(null, form)).toEqual({ name: 'alice', about: '  indented\n\n  text', birthday: { month: 4 } })
  })
})

describe('validateForm', () => {
  it('accepts an empty form and a full one', () => {
    expect(validateForm(emptyForm())).toEqual({})
    expect(validateForm(full())).toEqual({})
  })

  it('flags addresses that are not http(s), a malformed Lightning address and an impossible birthday', () => {
    const errors = validateForm({ ...emptyForm(), website: 'alice.example', picture: 'javascript:alert(1)', lud16: 'nope', birthday: { year: '', month: '13', day: '' } })
    expect(Object.keys(errors).sort()).toEqual(['birthday', 'lud16', 'picture', 'website'])
  })
})
