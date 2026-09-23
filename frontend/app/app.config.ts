import { defineAppConfig } from '#imports'

export default defineAppConfig({
  ui: {
    // `diogel` is the brand ramp defined in assets/css/main.css. Nuxt UI maps --color-primary-*
    // onto --ui-color-primary-* from this alias.
    colors: {
      primary: 'diogel',
      neutral: 'slate'
    }
  }
})
