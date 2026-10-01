// Starts and stops the compose stack in compose.e2e.yaml, and waits for the bunker to answer
// through the frontend proxy. Shared by global-setup.ts, global-teardown.ts and the
// `test:e2e:stack` script, which leaves the stack running for manual or Playwright MCP use.
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
// With the extension: stack-cli.ts runs this file directly under Node.
import { cookieJarPost, signInAs } from './sign-in.ts'

const here = dirname(fileURLToPath(import.meta.url))

export const COMPOSE_FILE = join(here, 'compose.e2e.yaml')
export const DEFAULT_PORT = '3100'

export function baseUrl(): string {
  return process.env.E2E_BASE_URL || `http://localhost:${process.env.E2E_PORT || DEFAULT_PORT}`
}

/**
 * The compose command, as argv. E2E_COMPOSE wins ("podman compose", "docker-compose"); otherwise
 * the first of docker compose, podman compose and docker-compose that answers `version`.
 */
export function composeCommand(): string[] {
  if (process.env.E2E_COMPOSE) {
    return process.env.E2E_COMPOSE.split(/\s+/).filter(Boolean)
  }

  const candidates = [['docker', 'compose'], ['podman', 'compose'], ['docker-compose']]
  for (const candidate of candidates) {
    const [bin, ...args] = candidate as [string, ...string[]]
    if (spawnSync(bin, [...args, 'version'], { stdio: 'ignore' }).status === 0) {
      return candidate
    }
  }

  throw new Error(
    'No compose command found. Install docker compose or podman compose, or set E2E_COMPOSE.'
  )
}

/** Runs `<compose> -f compose.e2e.yaml <args>`, inheriting this process's environment. */
export function compose(args: string[], options: { capture?: boolean } = {}): string {
  const [bin, ...prefix] = composeCommand() as [string, ...string[]]
  const output = execFileSync(bin, [...prefix, '-f', COMPOSE_FILE, ...args], {
    env: process.env,
    stdio: options.capture ? ['ignore', 'pipe', 'pipe'] : 'inherit',
    encoding: 'utf8'
  })
  return output ?? ''
}

/** Brings the stack up. E2E_BUILD=0 reuses images already tagged bancwr-*:e2e, as CI does. */
export function up(): void {
  const build = process.env.E2E_BUILD === '0' ? ['--no-build'] : ['--build']
  compose(['up', '--detach', ...build])
}

/** Saves the containers' logs beside the test results, then removes the stack and its volumes. */
export function down(): void {
  try {
    const logs = compose(['logs', '--no-color'], { capture: true })
    const dir = join(here, '..', 'test-results')
    mkdirSync(dir, { recursive: true })
    writeFileSync(join(dir, 'compose.log'), logs)
  } catch (error) {
    console.error('Could not save the e2e stack logs:', error)
  }

  compose(['down', '--volumes', '--remove-orphans'])
}

/**
 * Waits until an administrator can sign in and see the bunker healthy. That proves the whole path:
 * the Nuxt server, sign-in (which reaches the bunker as the service identity for its pubkey and the
 * vault lookup), the session, the signing proxy, and the bunker's guard.
 */
export async function waitForStack(adminNsec: string, timeoutMs = Number(process.env.E2E_READY_TIMEOUT || 120_000)) {
  const deadline = Date.now() + timeoutMs
  let last = 'no response'

  while (Date.now() < deadline) {
    try {
      const jar = cookieJarPost()
      const login = await signInAs(adminNsec, baseUrl(), jar.post)
      if (login.status === 200) {
        const response = await fetch(`${baseUrl()}/api/bunker/status`, { headers: { cookie: jar.cookie() } })
        const body = (await response.json()) as { status?: string }
        if (response.ok && body.status === 'healthy') {
          return
        }
        last = `status HTTP ${response.status} ${JSON.stringify(body.status)}`
      } else {
        last = `sign-in HTTP ${login.status} ${JSON.stringify(login.body)}`
      }
    } catch (error) {
      last = error instanceof Error ? error.message : String(error)
    }
    await new Promise(resolve => setTimeout(resolve, 1000))
  }

  throw new Error(`The stack was not ready at ${baseUrl()} after ${timeoutMs} ms (last: ${last}).`)
}
