import { generateNsec, npubFromNsec } from './keys'
import { baseUrl, down, up, waitForBunker } from './stack'

/**
 * Starts the e2e stack with a throwaway key, unless E2E_BASE_URL points at one that is already
 * running, and waits until the bunker answers through the frontend proxy.
 *
 * The specs read the bunker's expected npub from E2E_BUNKER_NPUB, which is set here: the workers
 * Playwright starts after global setup inherit this process's environment.
 */
export default async function globalSetup() {
  if (process.env.E2E_BASE_URL) {
    await waitForBunker()
    if (!process.env.E2E_BUNKER_NPUB) {
      // An external stack's key is known only if the caller passed it.
      process.env.E2E_BUNKER_NPUB = process.env.BUNKER_NSEC ? npubFromNsec(process.env.BUNKER_NSEC) : ''
    }
    return
  }

  process.env.BUNKER_NSEC = generateNsec()
  process.env.E2E_BUNKER_NPUB = npubFromNsec(process.env.BUNKER_NSEC)
  process.env.E2E_STACK_STARTED = '1'

  up()
  try {
    await waitForBunker()
  } catch (error) {
    // Playwright skips global teardown when setup throws, so clean up here.
    down()
    throw error
  }

  process.stdout.write(`e2e stack ready at ${baseUrl()} (bunker ${process.env.E2E_BUNKER_NPUB})\n`)
}
