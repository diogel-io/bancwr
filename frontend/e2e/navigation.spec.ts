import { test, expect } from './fixtures'

const pages = [
  { link: 'Dashboard', path: '/', title: 'Dashboard' },
  { link: 'Config', path: '/config', title: 'Bunker Configuration' },
  { link: 'Team', path: '/team', title: 'Team Management' },
  { link: 'Logs', path: '/logs', title: 'Signing Activity Log' }
]

test('the sidebar links reach every page', async ({ page }) => {
  await page.goto('/')
  const sidebar = page.getByRole('navigation')

  // Visit each page from the one before it, ending back on the dashboard.
  for (const target of [...pages.slice(1), pages[0]!]) {
    await sidebar.getByRole('link', { name: target.link, exact: true }).click()
    await expect(page).toHaveURL(target.path)
    await expect(page.getByRole('heading', { name: target.title })).toBeVisible()
  }
})

test('View All on the dashboard opens the activity log', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('link', { name: 'View All' }).click()
  await expect(page).toHaveURL('/logs')
  await expect(page.getByRole('heading', { name: 'Signing Activity Log' })).toBeVisible()
})

for (const target of pages) {
  test(`${target.path} renders on a direct load`, async ({ page }) => {
    const response = await page.goto(target.path)
    expect(response?.status()).toBe(200)
    await expect(page.getByRole('heading', { name: target.title })).toBeVisible()
  })
}
