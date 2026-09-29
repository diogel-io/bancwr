import { describe } from 'vitest'
import { nostrToolsSmokeTests } from './smoke'

// The Nuxt environment: loaded through Nuxt's Vite transform, as client code is.
describe('nostr-tools in the Nuxt environment', () => {
  nostrToolsSmokeTests()
})
