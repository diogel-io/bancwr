import { randomBytes } from 'node:crypto'
import { generateNsec, npubFromNsec } from './keys'
import { startRelay } from './relay'
import { startRemoteSigner } from './remote-signer'
import { cookieJarPost, signInAs } from './sign-in'
import { baseUrl, down, up, waitForStack } from './stack'

/**
 * Starts the e2e stack with throwaway keys and secrets (unless E2E_BASE_URL points at one already
 * running), waits until an administrator can sign in, then starts a relay and a NIP-46 remote
 * signer for the sign-in specs and registers that signer's key as a `user`, and registers one key
 * for each of the other roles (#26).
 *
 * The specs read what they need from the environment set here: the workers Playwright starts
 * after global setup inherit it.
 *   E2E_BUNKER_NPUB  the bunker's npub
 *   E2E_ADMIN_NSEC   the seeded administrator, which the signed-in fixture signs in as
 *   E2E_NIP46_URI    a bunker:// string for the remote signer, whose key is a registered `user`
 *   E2E_USER_NSEC    a registered `user`, for the role specs
 *   E2E_SIGNER_NSEC  a registered `signer`, for the role specs
 */
export default async function globalSetup() {
  if (process.env.E2E_BASE_URL) {
    // An external stack: its administrator key must be passed in.
    if (!process.env.E2E_ADMIN_NSEC) throw new Error('With E2E_BASE_URL, set E2E_ADMIN_NSEC to an administrator of that stack.')
    await waitForStack(process.env.E2E_ADMIN_NSEC)
    process.env.E2E_BUNKER_NPUB ??= process.env.BUNKER_NSEC ? npubFromNsec(process.env.BUNKER_NSEC) : ''
    await startNip46()
    await registerRoleKeys()
    return
  }

  process.env.BUNKER_NSEC = generateNsec()
  process.env.E2E_BUNKER_NPUB = npubFromNsec(process.env.BUNKER_NSEC)
  process.env.E2E_ADMIN_NSEC = generateNsec()
  process.env.E2E_ADMIN_NPUB = npubFromNsec(process.env.E2E_ADMIN_NSEC)
  process.env.E2E_PROXY_SECRET = randomBytes(32).toString('hex')
  process.env.E2E_SESSION_PASSWORD = randomBytes(32).toString('hex')
  process.env.E2E_STACK_STARTED = '1'

  up()
  try {
    await waitForStack(process.env.E2E_ADMIN_NSEC)
    await startNip46()
    await registerRoleKeys()
  } catch (error) {
    // Playwright skips global teardown when setup throws, so clean up here.
    await stopNip46()
    down()
    throw error
  }

  process.stdout.write(`e2e stack ready at ${baseUrl()} (bunker ${process.env.E2E_BUNKER_NPUB})\n`)
}

/** The relay and remote signer, held here for global-teardown.ts, which runs in this process. */
const handles = globalThis as { e2eNip46?: { stopRelay: () => Promise<void>, stopIndexer: () => Promise<void>, stopSigner: () => void } }

async function startNip46() {
  const port = Number(process.env.E2E_RELAY_PORT || 7777)
  const relay = `ws://127.0.0.1:${port}`
  const stopRelay = await startRelay(port)
  // A second relay standing in for the indexer relays the profile page also asks (#62).
  const stopIndexer = await startRelay(Number(process.env.E2E_INDEXER_PORT || 7778))
  const signerNsec = generateNsec()
  const signer = startRemoteSigner(signerNsec, relay)
  handles.e2eNip46 = { stopRelay, stopIndexer, stopSigner: signer.stop }

  // Register the signer's key as a user, signed in as the administrator.
  const jar = cookieJarPost()
  await signInAs(process.env.E2E_ADMIN_NSEC!, baseUrl(), jar.post)
  const added = await jar.post(`${baseUrl()}/api/bunker/team`, {
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: 'NIP-46 test signer', pubkey: signer.pubkey, role: 'user' })
  })
  if (added.status !== 200) throw new Error(`Could not register the NIP-46 test signer: HTTP ${added.status}`)

  process.env.E2E_NIP46_URI = `bunker://${signer.pubkey}?relay=${encodeURIComponent(relay)}`
}

/** One throwaway key for each role below administrator, registered by the administrator. */
async function registerRoleKeys() {
  const jar = cookieJarPost()
  await signInAs(process.env.E2E_ADMIN_NSEC!, baseUrl(), jar.post)
  for (const [role, variable] of [['user', 'E2E_USER_NSEC'], ['signer', 'E2E_SIGNER_NSEC']] as const) {
    const nsec = generateNsec()
    const added = await jar.post(`${baseUrl()}/api/bunker/team`, {
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: `e2e ${role}`, pubkey: npubFromNsec(nsec), role })
    })
    if (added.status !== 200) throw new Error(`Could not register the e2e ${role}: HTTP ${added.status}`)
    process.env[variable] = nsec
  }
}

export async function stopNip46() {
  handles.e2eNip46?.stopSigner()
  await handles.e2eNip46?.stopRelay()
  await handles.e2eNip46?.stopIndexer()
  handles.e2eNip46 = undefined
}
