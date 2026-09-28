import withNuxt from './.nuxt/eslint.config.mjs'

export default withNuxt([
  {
    rules: {
      // Project-specific overrides
      'vue/multi-word-component-names': 'off', // Allow index.vue, config.vue
      'no-unused-vars': 'off', // Handle in TS or let it be for now
      '@typescript-eslint/no-unused-vars': 'off', // Handle in TS or let it be for now
      'no-console': ['warn', { allow: ['error'] }], // Warn on console.log, allow console.error
      '@typescript-eslint/no-explicit-any': 'off'
    }
  },
  {
    // JetBrains HTTP Client response handlers, run by the .http files under tests/api. That
    // runtime supplies these globals; they are not browser or Node scripts.
    files: ['tests/api/asserts/**/*.js'],
    languageOptions: {
      globals: {
        client: 'readonly',
        response: 'readonly'
      }
    }
  }
])
