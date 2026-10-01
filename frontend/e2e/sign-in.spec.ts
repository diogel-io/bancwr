// Sign-in (#11), through the real stack: every acceptance criterion of diogel-io/bancwr#11.
import type { Page } from '@playwright/test'
import { installExtension } from './extension'
import { anonymousTest as test, expect } from './fixtures'
import { generateNsec, npubFromNsec } from './keys'

const signInWithExtension = (page: Page) => page.getByRole('button', { name: 'Sign in with extension' }).click()

test('every page sends a visitor with no session to sign-in, typed URLs included', async ({ page }) => {
  for (const path of ['/', '/config', '/team', '/logs', '/no-access']) {
    await page.goto(path)
    await expect(page, path).toHaveURL('/sign-in')
  }
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible()
})

test('without an extension, NIP-07 is explained and NIP-46 stays available', async ({ page }) => {
  await page.goto('/sign-in')
  await expect(page.getByText('No Nostr signing extension was found')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Sign in with extension' })).toHaveCount(0)
  await expect(page.getByPlaceholder('bunker://…')).toBeVisible()
})

test('a registered key signs in with NIP-07, and signing out ends the session for good', async ({ page, context }) => {
  await installExtension(page, process.env.E2E_ADMIN_NSEC!)
  await page.goto('/sign-in')
  await signInWithExtension(page)

  await expect(page).toHaveURL('/')
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible()
  const admin = npubFromNsec(process.env.E2E_ADMIN_NSEC!)
  const identity = page.getByRole('group', { name: /^Signed in as / })
  await expect(identity).toHaveAttribute('aria-label', `Signed in as ${admin}, Administrator`)

  const session = (await context.cookies()).find(c => c.name === 'bancwr-session')!
  expect(session).toMatchObject({ httpOnly: true, secure: true, sameSite: 'Strict' })

  await page.getByRole('button', { name: 'Sign out' }).click()
  await expect(page).toHaveURL('/sign-in')

  // The cookie from before sign-out no longer works, even when put back.
  await context.addCookies([session])
  await page.goto('/team')
  await expect(page).toHaveURL('/sign-in')
  expect((await page.request.get('/api/bunker/team')).status()).toBe(401)
})

test('an unregistered key reaches only the no-access page, which shows its npub', async ({ page }) => {
  const stranger = generateNsec()
  await installExtension(page, stranger)
  await page.goto('/sign-in')
  await signInWithExtension(page)

  await expect(page).toHaveURL('/no-access')
  await expect(page.getByText('This key is not registered with this bunker')).toBeVisible()
  await expect(page.getByTestId('presented-npub')).toHaveText(npubFromNsec(stranger))
  await expect(page.getByText('vault administrator')).toBeVisible()

  for (const path of ['/', '/team', '/sign-in']) {
    await page.goto(path)
    await expect(page, path).toHaveURL('/no-access')
  }

  await page.getByRole('button', { name: 'Sign out and try another key' }).click()
  await expect(page).toHaveURL('/sign-in')
})

test('a registered key signs in through a NIP-46 remote signer', async ({ page }) => {
  await page.goto('/sign-in')
  await page.getByPlaceholder('bunker://…').fill(process.env.E2E_NIP46_URI!)
  await page.getByRole('button', { name: 'Connect and sign in' }).click()

  await expect(page).toHaveURL('/', { timeout: 30_000 })
  await expect(page.getByRole('group', { name: /^Signed in as / })).toHaveAttribute('aria-label', /, User$/)
})

test('a bunker:// string that is not one is refused before anything is contacted', async ({ page }) => {
  await page.goto('/sign-in')
  await page.getByPlaceholder('bunker://…').fill('alice@example.com')
  await page.getByRole('button', { name: 'Connect and sign in' }).click()
  await expect(page.getByTestId('sign-in-error')).toContainText('bunker://')
})
