import { describe, it, expect, beforeEach } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { createError } from 'h3'
import AppNavbar from '~/components/AppNavbar.vue'
import Index from '~/pages/index.vue'
import Config from '~/pages/config.vue'
import Logs from '~/pages/logs.vue'
import Team from '~/pages/team/index.vue'
import TeamMember from '~/pages/team/[pubkey].vue'
import { signInAs } from '../helpers/session'
import ErrorPage from '~/error.vue'

registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0', checks: [] }))
registerEndpoint('/api/bunker/config', () => ({ pubkey: 'npub1test', nsec_file: null }))
registerEndpoint('/api/bunker/logs', () => [])
registerEndpoint('/api/bunker/team', () => [])
registerEndpoint('/api/bunker/metrics', () => ({ http_requests: 0, nip46_connections: 0, total_signatures: 0 }))
registerEndpoint('/api/version', () => ({ version: '0.1.0' }))

// One navbar on every page (#28), so the health indicator cannot be missing from one.
describe('every page renders through AppNavbar', () => {
  // Team's title is the administrator's (a viewer's list is "Team", #77).
  beforeEach(() => signInAs('administrator'))
  const pages = [
    ['Dashboard', Index],
    ['Bunker Configuration', Config],
    ['Signing Activity Log', Logs],
    ['Team Management', Team],
    ['Team member', TeamMember]
  ] as const

  for (const [title, page] of pages) {
    it(title, async () => {
      const component = await mountSuspended(page)
      const navbar = component.findComponent(AppNavbar)

      expect(navbar.exists()).toBe(true)
      expect(navbar.props('title')).toBe(title)
      expect(component.find('[data-testid="bunker-health"]').exists()).toBe(true)
    })
  }

  it('the error page', async () => {
    const error = createError({ status: 404, statusText: 'Page not found: /nowhere' })
    const component = await mountSuspended(ErrorPage, { props: { error } })

    expect(component.findComponent(AppNavbar).props('title')).toBe('Error 404')
  })
})
