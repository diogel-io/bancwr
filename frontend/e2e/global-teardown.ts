import { stopNip46 } from './global-setup'
import { down } from './stack'

/** Stops the relay and remote signer, and removes the stack global-setup.ts started. */
export default async function globalTeardown() {
  await stopNip46()
  if (process.env.E2E_STACK_STARTED === '1') {
    down()
  }
}
