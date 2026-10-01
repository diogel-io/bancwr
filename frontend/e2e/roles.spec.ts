import { test, expect, type Role } from './fixtures'

// What each role is offered and refused (#26). The bunker enforces the same matrix (#25); these
// check the UI neither offers nor opens what it would refuse.
const ADMIN_PAGES = [
  { link: 'Config', path: '/config', title: 'Config' },
  { link: 'Team', path: '/team', title: 'Team' },
  { link: 'Logs', path: '/logs', title: 'Logs' }
]

test.describe('administrator', () => {
  test('is offered every page, and sees the whole dashboard', async ({ page }) => {
    await page.goto('/')
    const sidebar = page.getByRole('navigation')
    for (const name of ['Dashboard', 'Config', 'Team', 'Logs']) {
      await expect(sidebar.getByRole('link', { name, exact: true })).toBeVisible()
    }
    await expect(page.getByText('Total Signatures')).toBeVisible()
    await expect(page.getByText('Recent Activity')).toBeVisible()
  })
})

for (const role of ['user', 'signer'] as Role[]) {
  test.describe(role, () => {
    test.use({ role })

    test('is offered the dashboard only', async ({ page }) => {
      await page.goto('/')
      const links = page.getByRole('navigation').getByRole('link')
      await expect(links).toHaveCount(1)
      await expect(links.first()).toHaveAccessibleName('Dashboard')
    })

    test('sees the bunker\'s health only, and the page asks the bunker for nothing it refuses', async ({ page }) => {
      const refused: string[] = []
      page.on('response', (response) => {
        if (response.url().includes('/api/bunker/') && response.status() === 403) refused.push(response.url())
      })

      const response = await page.goto('/')
      expect(response?.status()).toBe(200)
      await expect(page.getByText('Bunker Status')).toBeVisible()
      await expect(page.getByText('healthy')).toBeVisible()
      await expect(page.getByTestId('role-summary')).toContainText(role === 'user' ? 'User' : 'Signer')
      await expect(page.getByText('Total Signatures')).toHaveCount(0)
      await expect(page.getByText('Recent Activity')).toHaveCount(0)
      await page.waitForLoadState('networkidle')
      expect(refused).toEqual([])
    })

    for (const target of ADMIN_PAGES) {
      test(`typing ${target.path} gets the permission-denied page`, async ({ page }) => {
        const response = await page.goto(target.path)
        expect(response?.status()).toBe(403)
        await expect(page.getByRole('heading', { name: `You don't have access to ${target.title}` })).toBeVisible()
        await expect(page.getByTestId('permission-denied')).toContainText(role === 'user' ? 'User' : 'Signer')
        // The sidebar stays, and the way out works.
        await expect(page.getByRole('navigation').getByRole('link', { name: 'Dashboard', exact: true })).toBeVisible()
        await page.getByRole('button', { name: 'Go to the dashboard' }).click()
        await expect(page).toHaveURL('/')
        await expect(page.getByText('Bunker Status')).toBeVisible()
      })
    }
  })
}
