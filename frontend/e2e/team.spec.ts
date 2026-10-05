import type { Page } from '@playwright/test'
import { test, expect } from './fixtures'
import { npubToHex, randomNpub } from './keys'

// One bunker database serves the whole run, so every member gets a name no other test uses.
let counter = 0
const uniqueName = (label: string) => `${label} ${Date.now()}-${++counter}`

test.beforeEach(async ({ page }) => {
  await page.goto('/team')
})

test('adds a member through the bunker', async ({ page }) => {
  const name = uniqueName('Alice')
  const pubkey = randomNpub()

  await addMember(page, name, pubkey, 'Admin')

  await expect(page.getByText('Member added successfully', { exact: true })).toBeVisible()
  await expect(memberRow(page, name)).toContainText(pubkey)
  await expect(memberRow(page, name)).toContainText('Admin')

  // The form is ready for the next member.
  await expect(page.getByLabel('Name')).toHaveValue('')
  await expect(page.getByLabel('Pubkey')).toHaveValue('')

  // Stored by the bunker, not just shown: it survives a reload.
  await page.reload()
  await expect(memberRow(page, name)).toContainText(pubkey)
})

test('removes a member once confirmed (#43)', async ({ page }) => {
  const name = uniqueName('Bob')
  await addMember(page, name, randomNpub(), 'Signer')
  await expect(memberRow(page, name)).toBeVisible()

  page.once('dialog', dialog => dialog.accept())
  await page.getByRole('button', { name: `Remove ${name}` }).click()

  await expect(page.getByText('Member removed', { exact: true })).toBeVisible()
  await expect(memberRow(page, name)).toHaveCount(0)

  await page.reload()
  await expect(page.getByRole('heading', { name: 'Team Management' })).toBeVisible()
  await expect(memberRow(page, name)).toHaveCount(0)
})

test('keeps a member when removal is cancelled', async ({ page }) => {
  const name = uniqueName('Carol')
  await addMember(page, name, randomNpub(), 'Viewer')
  await expect(memberRow(page, name)).toBeVisible()

  let message = ''
  page.once('dialog', (dialog) => {
    message = dialog.message()
    return dialog.dismiss()
  })
  await page.getByRole('button', { name: `Remove ${name}` }).click()

  expect(message).toBe(`Are you sure you want to remove ${name}?`)
  await page.reload()
  await expect(memberRow(page, name)).toBeVisible()
})

test('offers exactly the three roles (#77)', async ({ page }) => {
  await page.getByRole('combobox').click()
  await expect(page.getByRole('option')).toHaveText(['Admin', 'Signer', 'Viewer'])
})

test('opens a member\'s read-only profile from their row (#77)', async ({ page }) => {
  const name = uniqueName('Erin')
  const pubkey = randomNpub()
  await addMember(page, name, pubkey, 'Signer')
  await page.getByRole('link', { name: `View ${name}'s profile` }).click()

  await expect(page).toHaveURL(`/team/${pubkey}`)
  await expect(page.getByTestId('member-summary')).toContainText(name)
  await expect(page.getByTestId('member-role')).toHaveText('Signer')
  await expect(page.getByTestId('member-profile-not-found')).toBeVisible({ timeout: 20_000 })
})

test('refuses the same key twice, even as hex (#24)', async ({ page }) => {
  const name = uniqueName('Dave')
  const key = randomNpub()
  await addMember(page, name, key, 'Viewer')
  await expect(memberRow(page, name)).toBeVisible()

  const refused = page.waitForResponse(response =>
    response.url().endsWith('/api/bunker/team') && response.request().method() === 'POST'
  )
  await addMember(page, uniqueName('Dave again'), npubToHex(key), 'Signer')
  expect((await refused).status()).toBe(409)
  await expect(page.getByText('Failed to add member', { exact: true })).toBeVisible()
})

test('rejects a pubkey that is not an npub', async ({ page }) => {
  const name = uniqueName('Mallory')
  const rejected = page.waitForResponse(response =>
    response.url().endsWith('/api/bunker/team') && response.request().method() === 'POST'
  )

  await addMember(page, name, 'not-a-pubkey', 'Signer')

  expect((await rejected).status()).toBe(400)
  await expect(page.getByText('Failed to add member', { exact: true })).toBeVisible()
  await expect(memberRow(page, name)).toHaveCount(0)
})

async function addMember(page: Page, name: string, pubkey: string, role: string) {
  await page.getByLabel('Name').fill(name)
  await page.getByLabel('Pubkey').fill(pubkey)
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: role, exact: true }).click()
  await page.getByRole('button', { name: 'Add Member' }).click()
}

function memberRow(page: Page, name: string) {
  return page.getByRole('row').filter({ hasText: name })
}
