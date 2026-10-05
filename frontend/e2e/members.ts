// A fresh key registered by the administrator, for specs that must not touch the shared role keys'
// profiles or lists (#72, #75).
import { expect } from '@playwright/test'
import { generateNsec, npubFromNsec } from './keys'
import { cookieJarPost, signInAs } from './sign-in'
import { baseUrl } from './stack'

export async function newMember(role: 'signer' | 'viewer' = 'signer'): Promise<string> {
  const nsec = generateNsec()
  const jar = cookieJarPost()
  await signInAs(process.env.E2E_ADMIN_NSEC!, baseUrl(), jar.post)
  const added = await jar.post(`${baseUrl()}/api/bunker/team`, {
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: `e2e member ${Date.now()}`, pubkey: npubFromNsec(nsec), role })
  })
  expect(added.status).toBe(200)
  return nsec
}
