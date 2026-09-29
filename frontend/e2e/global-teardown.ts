import { down } from './stack'

/** Removes the stack global-setup.ts started. An external stack (E2E_BASE_URL) is left alone. */
export default async function globalTeardown() {
  if (process.env.E2E_STACK_STARTED === '1') {
    down()
  }
}
