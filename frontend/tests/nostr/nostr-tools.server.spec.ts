// @vitest-environment node
import { describe } from 'vitest'
import { nostrToolsSmokeTests } from './smoke'

// Plain Node, as server-side rendering loads it: the ESM subpath exports and @noble/* must
// resolve without a browser. Relay I/O is not covered: it is client-only (see the README).
describe('nostr-tools in Node', () => {
  nostrToolsSmokeTests()
})
