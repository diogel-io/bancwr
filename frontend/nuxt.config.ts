// https://nuxt.com/docs/api/configuration/nuxt-config
import {defineNuxtConfig} from 'nuxt/config'

export default defineNuxtConfig({
    runtimeConfig: {
        // Only the /api/bunker proxy reads this, so it stays private: a public key would ship the
        // bunker's address in the client payload for no reason, and the NUXT_PUBLIC_ prefix it
        // required is what made compose.yaml's variable name look plausible. See #20.
        apiBase: process.env.NUXT_API_BASE || 'http://localhost:3000',
        // NUXT_PROXY_SECRET: shared with the bunker (BANCWR_PROXY_SECRET) to sign who is calling
        // (#25). Private, like apiBase. Sign-in (#11) makes it required.
        proxySecret: '',
        // NUXT_SESSION_PASSWORD: seals the session cookie, 32+ characters (#11).
        sessionPassword: '',
        // NUXT_SITE_ORIGIN: the https origin users reach Bancwr on, which sign-in events must name.
        // Never taken from the request's Host header (#11).
        siteOrigin: '',
        public: {
            // NUXT_PUBLIC_PROFILE_RELAYS: comma-separated relays the profile page reads and publishes
            // through, besides the member's own NIP-65 write relays (#30). Empty means the defaults
            // in app/utils/profile-relays.ts, so compose can pass it through unset.
            profileRelays: '',
            // NUXT_PUBLIC_INDEXER_RELAYS: comma-separated indexer relays, which collect profiles and
            // relay lists from across the network. The profile page looks members up there too, and
            // publishes there (#62). Empty means the defaults in app/utils/profile-relays.ts.
            indexerRelays: '',
            // NUXT_PUBLIC_BLOSSOM_SERVER: where profile images are uploaded when the member has no
            // Blossom server list of their own (#30). Empty means the default, likewise.
            blossomServer: ''
        }
    },
    modules: [
        '@nuxt/eslint',
        '@nuxt/ui'
    ],
    devtools: {enabled: true},
    // Cross-origin requests carry no Referer, so third-party hosts (profile pictures, NIP-05,
    // Blossom) aren't told which Bancwr is calling (#75). Bancwr's own requests keep theirs. The
    // meta tag below says the same, in case a proxy in front drops the header; third-party images
    // also set it themselves (utils/no-referrer-img.ts).
    routeRules: {
        '/**': {headers: {'Referrer-Policy': 'same-origin'}}
    },
    app: {
        head: {
            meta: [
                {name: 'referrer', content: 'same-origin'}
            ],
            link: [
                // The Diogel mark. The SVG follows the browser's colour scheme; the .ico cannot, and
                // carries 16/32/48 for the contexts that fall back to it.
                {rel: 'icon', type: 'image/svg+xml', href: '/favicon.svg'},
                {rel: 'shortcut icon', type: 'image/x-icon', href: '/favicon.ico'},
                {rel: 'apple-touch-icon', sizes: '180x180', href: '/apple-touch-icon.png'}
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
    vite: {
        optimizeDeps: {
            include: [
                '@vue/devtools-core',
                '@vue/devtools-kit',
            ]
        }
    }
})
