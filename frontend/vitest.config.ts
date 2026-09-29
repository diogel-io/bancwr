import { configDefaults } from 'vitest/config'
import { defineVitestConfig } from '@nuxt/test-utils/config'

export default defineVitestConfig({
  test: {
    environment: 'nuxt',
    // e2e/ holds the Playwright suite, which `pnpm test:e2e` runs against the real stack.
    exclude: [...configDefaults.exclude, 'e2e/**']
  }
})
