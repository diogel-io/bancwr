// https://nuxt.com/docs/api/configuration/nuxt-config
import { defineNuxtConfig } from 'nuxt/config'

export default defineNuxtConfig({
  runtimeConfig: {
    // Only the /api/bunker proxy reads this, so it stays private: a public key would ship the
    // bunker's address in the client payload for no reason, and the NUXT_PUBLIC_ prefix it
    // required is what made compose.yaml's variable name look plausible. See #20.
    apiBase: process.env.NUXT_API_BASE || 'http://localhost:3000'
  },
  modules: [
    '@nuxt/eslint',
    '@nuxt/ui',
    '@nuxt/image',
    '@nuxtjs/mdc'
  ],
  devtools: { enabled: true },
  app: {
    head: {
      link: [
        // The Diogel mark. The SVG follows the browser's colour scheme; the .ico cannot, and
        // carries 16/32/48 for the contexts that fall back to it.
        { rel: 'icon', type: 'image/svg+xml', href: '/favicon.svg' },
        { rel: 'shortcut icon', type: 'image/x-icon', href: '/favicon.ico' },
        { rel: 'apple-touch-icon', sizes: '180x180', href: '/apple-touch-icon.png' }
      ]
    }
  },
  css: ['~/assets/css/main.css'],
  devServer: {
    port: 3001
  },
  compatibilityDate: '2025-07-15',
})
