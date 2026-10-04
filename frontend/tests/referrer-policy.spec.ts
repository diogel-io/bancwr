// @vitest-environment node
// Every third-party image sets its referrer policy before `src` (#75), so its host isn't told which
// Bancwr shows it. `referrerpolicy` as a plain attribute comes too late on an element whose `src`
// is set first (#72's e2e test saw the Referer sent), so images go through NoReferrerImg
// (app/utils/no-referrer-img.ts). This reads the templates, so a new image that forgets fails here.
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { describe, it, expect } from 'vitest'

const APP = resolve(__dirname, '../app')

/** Images that never load from another host, so need no policy. */
const LOCAL_IMAGES: Record<string, string> = {
  'components/DiogelLogo.vue': 'bundled logo files, served by Bancwr itself',
  'components/connections/IssueToken.vue': 'the QR code is a data: URL generated in the browser'
}

function vueFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) return vueFiles(path)
    return name.endsWith('.vue') ? [path] : []
  })
}

/** Each opening tag named `name` in the template, attributes included; quotes may contain `>`. */
export function openingTags(source: string, name: string): string[] {
  const tags: string[] = []
  const pattern = new RegExp(`<${name}(?=[\\s/>])`, 'g')
  for (let match = pattern.exec(source); match; match = pattern.exec(source)) {
    let quote: string | undefined
    let end = match.index
    for (; end < source.length; end++) {
      const char = source[end]!
      if (quote) {
        if (char === quote) quote = undefined
      } else if (char === '"' || char === '\'') {
        quote = char
      } else if (char === '>') {
        break
      }
    }
    tags.push(source.slice(match.index, end + 1))
  }
  return tags
}

const dynamicSrc = (tag: string) => /\s(?::|v-bind:)src=/.test(tag)

export function violations(files: Record<string, string>): string[] {
  const found: string[] = []
  for (const [file, source] of Object.entries(files)) {
    const template = source.slice(source.indexOf('<template'))
    for (const tag of openingTags(template, 'UAvatar')) {
      if (dynamicSrc(tag) && !/\s:as=/.test(tag)) found.push(`${file}: UAvatar with :src but no :as="{ img: NoReferrerImg }"`)
    }
    if (file in LOCAL_IMAGES) continue
    for (const tag of openingTags(template, 'img')) {
      if (dynamicSrc(tag)) found.push(`${file}: <img :src> instead of <NoReferrerImg :src>`)
    }
  }
  return found
}

describe('third-party images send no Referer (#75)', () => {
  const files = Object.fromEntries(vueFiles(APP).map(path => [relative(APP, path), readFileSync(path, 'utf8')]))

  it('reads the templates, so it cannot pass by reading nothing', () => {
    expect(Object.keys(files).length).toBeGreaterThan(20)
    expect(Object.values(files).some(source => openingTags(source, 'UAvatar').length > 0)).toBe(true)
  })

  it('finds every UAvatar and dynamic <img> going through NoReferrerImg', () => {
    expect(violations(files)).toEqual([])
  })

  it('catches an image that forgets', () => {
    expect(violations({
      'components/New.vue': '<template><UAvatar :src="p" alt="x" /><img\n  :src="u"\n  alt="a > b"\n></template>'
    })).toEqual([
      'components/New.vue: UAvatar with :src but no :as="{ img: NoReferrerImg }"',
      'components/New.vue: <img :src> instead of <NoReferrerImg :src>'
    ])
  })

  it('leaves static and local images alone', () => {
    expect(violations({
      'components/Static.vue': '<template><img src="/logo.svg" alt=""><UAvatar icon="i-lucide-user" /></template>',
      'components/DiogelLogo.vue': '<template><img :src="lightLogoUrl" alt="Diogel"></template>'
    })).toEqual([])
  })

  it('keeps the allowlist to files that exist', () => {
    for (const file of Object.keys(LOCAL_IMAGES)) expect(files[file], file).toBeDefined()
  })
})
