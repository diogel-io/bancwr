// https://nuxt.com/docs/api/configuration/nuxt-config
import { defineNuxtConfig } from 'nuxt/config'

export default defineNuxtConfig({
  runtimeConfig: {
    // Only the /api/bunker proxy reads this, so it stays private: a public key would ship the
    // bunker's address in the client payload for no reason, and the NUXT_PUBLIC_ prefix it
    // required is what made compose.yaml's variable name look plausible. See #20.
    apiBase: process.env.NUXT_API_BASE || 'http://localhost:3000',
    // SPIKE (#23). All three are required in production; none has a usable default.
    // NUXT_SESSION_PASSWORD: seals the session cookie, 32+ characters.
    sessionPassword: '',
    // NUXT_PROXY_SECRET: shared with the bunker (BANCWR_PROXY_SECRET) to sign proxied identity.
    proxySecret: '',
    // NUXT_SITE_ORIGIN: the origin users reach Bancwr on, e.g. https://bancwr.example. Sign-in
    // events must name it; the request's own Host header is never trusted for this.
    siteOrigin: ''
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
  typescript: {
    // The vitest suites run in the nuxt environment, so they are type-checked as app code.
    tsConfig: {
      include: ['../tests/**/*.ts']
    }
  },
  compatibilityDate: '2025-07-15',
})
