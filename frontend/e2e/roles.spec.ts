import type { Page } from '@playwright/test'
import { test, expect, sameOrigin } from './fixtures'
import { randomNpub } from './keys'

// What each role is offered and refused (#77, which replaced #26's roles; #143). The bunker enforces the
// same matrix (#25); these check the UI neither offers nor opens what it would refuse, and that a
// page never asks the bunker for something it then refuses.

/** Collects every /api/bunker/* answer that was a 403, to show a page asks only for what it may. */
function watchRefusals(page: Page): string[] {
  const refused: string[] = []
  page.on('response', (response) => {
    if (response.url().includes('/api/bunker/') && response.status() === 403) refused.push(response.url())
  })
  return refused
}

async function expectSidebar(page: Page, names: string[]) {
  const links = page.getByRole('navigation').getByRole('link')
  await expect(links).toHaveCount(names.length)
  for (const [i, name] of names.entries()) await expect(links.nth(i)).toHaveAccessibleName(name)
}

async function expectRefused(page: Page, path: string, title: string, roleLabel: string) {
  const response = await page.goto(path)
  expect(response?.status()).toBe(403)
  await expect(page.getByRole('heading', { name: `You don't have access to ${title}` })).toBeVisible()
  await expect(page.getByTestId('permission-denied')).toContainText(roleLabel)
  // The sidebar stays, and the way out works.
  await expect(page.getByRole('navigation').getByRole('link', { name: 'Dashboard', exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'Go to the dashboard' }).click()
  await expect(page).toHaveURL('/')
  await expect(page.getByText('Bunker Status')).toBeVisible()
}

test.describe('admin', () => {
  test('is offered every page, and sees the whole dashboard', async ({ page }) => {
    await page.goto('/')
    await expectSidebar(page, ['Dashboard', 'Config', 'Team', 'Logs', 'Profile', 'Follows', 'Relays', 'Connections', 'Approvals'])
    await expect(page.getByText('Total Signatures')).toBeVisible()
    await expect(page.getByText('Recent Activity')).toBeVisible()
  })

  test('manages the team, and opens a member\'s profile', async ({ page }) => {
    await page.goto('/team')
    await expect(page.getByRole('heading', { name: 'Team Management' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Add Member' })).toBeVisible()
    await page.getByRole('link', { name: 'View e2e signer\'s profile' }).click()
    await expect(page).toHaveURL(/\/team\/npub1/)
    await expect(page.getByTestId('member-summary')).toContainText('e2e signer')
    await expect(page.getByTestId('member-role')).toHaveText('Signer')
    // Read-only: whatever the relays hold, there is nothing to edit.
    await expect(page.getByTestId('member-profile-loading')).toHaveCount(0, { timeout: 20_000 })
    await expect(page.locator('input, textarea')).toHaveCount(0)
  })
})

test.describe('signer', () => {
  test.use({ role: 'signer' })

  test('cannot export the log or change its retention (#96)', async ({ page }) => {
    await page.goto('/')
    const exported = await page.request.get('/api/bunker/logs/export?format=csv&from=2026-01-01&to=2026-12-31')
    expect(exported.status()).toBe(403)
    const retention = await page.request.put('/api/bunker/logs/retention', { headers: sameOrigin, data: { max_age_days: 30, max_rows: null } })
    expect(retention.status()).toBe(403)
  })

  test('is offered the dashboard, their profile, follows, relays, connections and approvals', async ({ page }) => {
    await page.goto('/')
    await expectSidebar(page, ['Dashboard', 'Profile', 'Follows', 'Relays', 'Connections', 'Approvals'])
  })

  test('sees the health, their connected apps and their own signatures, and nothing it would be refused', async ({ page }) => {
    const refused = watchRefusals(page)
    const response = await page.goto('/')
    expect(response?.status()).toBe(200)
    await expect(page.getByText('Bunker Status')).toBeVisible()
    await expect(page.getByText('healthy', { exact: true })).toBeVisible()
    await expect(page.getByTestId('role-summary')).toContainText('Signer')
    await expect(page.getByTestId('my-connections')).toBeVisible()
    await expect(page.getByTestId('my-connections-count')).toHaveText(/^\d+$/)
    await expect(page.getByTestId('my-signatures')).toBeVisible()
    await expect(page.getByText('Total Signatures')).toHaveCount(0)
    await expect(page.getByText('Recent Activity')).toHaveCount(0)
    await page.waitForLoadState('networkidle')
    expect(refused).toEqual([])
  })

  // The team and members' profiles are the administrators' alone (#143).
  for (const [path, title] of [
    ['/config', 'Config'], ['/logs', 'Logs'], ['/team', 'Team'],
    [`/team/${randomNpub()}`, 'a team member\'s profile']
  ] as const) {
    test(`typing ${path.startsWith('/team/') ? '/team/<npub>' : path} gets the permission-denied page`, async ({ page }) => {
      await expectRefused(page, path, title, 'Signer')
    })
  }

  test('cannot read or add to the team through the API either', async ({ page }) => {
    expect((await page.request.get('/api/bunker/team')).status()).toBe(403)
    const added = await page.request.post('/api/bunker/team', { headers: sameOrigin, data: { name: 'Nope', pubkey: 'npub1nope', role: 'signer' } })
    expect(added.status()).toBe(403)
  })
})
