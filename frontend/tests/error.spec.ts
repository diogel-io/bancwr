import { describe, it, expect } from 'vitest'
import { mountSuspended, registerEndpoint } from '@nuxt/test-utils/runtime'
import { createError } from 'h3'
import { signInAs } from './helpers/session'
import ErrorPage from '~/error.vue'

describe('error page', () => {
  // The default layout's footer asks for both versions (#35).
  registerEndpoint('/api/bunker/status', () => ({ status: 'healthy', pubkey: 'npub1test', version: '0.1.0' }))
  registerEndpoint('/api/version', () => ({ version: '0.1.0' }))

  it('says which page the role cannot open, names the role, and links to the dashboard (#26)', async () => {
    signInAs('signer')
    const error = createError({ status: 403, statusText: 'Forbidden', data: { reason: 'forbidden_route', path: '/config' } })
    const component = await mountSuspended(ErrorPage, { props: { error } })

    const denied = component.find('[data-testid="permission-denied"]')
    expect(denied.text()).toContain('You don\'t have access to Config')
    expect(denied.text()).toContain('signed in as Signer')
    expect(component.findAll('button').some(b => b.text().includes('Go to the dashboard'))).toBe(true)
    // Inside the default layout, so the sidebar stays.
    expect(component.findAll('a').map(a => a.attributes('href'))).toContain('/')
  })

  it('reads the reason from server-rendered errors, whose data arrives as a string', async () => {
    const error = createError({ status: 403, data: JSON.stringify({ reason: 'forbidden_route', path: '/team' }) })
    const component = await mountSuspended(ErrorPage, { props: { error } })

    expect(component.find('[data-testid="permission-denied"]').text()).toContain('You don\'t have access to Team')
  })

  it('keeps the usual status and message for any other error', async () => {
    const error = createError({ status: 404, statusText: 'Page not found: /nowhere' })
    const component = await mountSuspended(ErrorPage, { props: { error } })

    expect(component.find('[data-testid="permission-denied"]').exists()).toBe(false)
    expect(component.text()).toContain('404')
    expect(component.text()).toContain('Page not found: /nowhere')
  })

  it('does not treat a bunker 403 without the route reason as a route refusal', async () => {
    const error = createError({ status: 403, statusText: 'Forbidden' })
    const component = await mountSuspended(ErrorPage, { props: { error } })

    expect(component.find('[data-testid="permission-denied"]').exists()).toBe(false)
    expect(component.text()).toContain('403')
  })
})
