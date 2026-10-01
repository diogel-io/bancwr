// `pnpm test:e2e:stack` / `pnpm test:e2e:stack:down`: runs the e2e stack on its own, for driving
// it by hand or through the Playwright MCP server in ../../.mcp.json. Node runs this file directly
// (type stripping), hence the .ts import specifiers.
//
// Sign-in is required (#11). The stack seeds a throwaway administrator, whose nsec is printed for
// test use only. To sign in with your own extension instead, pass your npub as E2E_ADMIN_PUBKEY
// and it is registered as an administrator too.
import { randomBytes } from 'node:crypto'
import { generateNsec, npubFromNsec } from './keys.ts'
import { cookieJarPost, signInAs } from './sign-in.ts'
import { baseUrl, down, up, waitForStack } from './stack.ts'

const command = process.argv[2]

if (command === 'up') {
  process.env.BUNKER_NSEC = generateNsec()
  const adminNsec = generateNsec()
  process.env.E2E_ADMIN_NPUB = npubFromNsec(adminNsec)
  process.env.E2E_PROXY_SECRET = randomBytes(32).toString('hex')
  process.env.E2E_SESSION_PASSWORD = randomBytes(32).toString('hex')
  up()
  await waitForStack(adminNsec)

  if (process.env.E2E_ADMIN_PUBKEY) {
    const jar = cookieJarPost()
    await signInAs(adminNsec, baseUrl(), jar.post)
    const added = await jar.post(`${baseUrl()}/api/bunker/team`, {
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'You', pubkey: process.env.E2E_ADMIN_PUBKEY, role: 'administrator' })
    })
    process.stdout.write(`Registered E2E_ADMIN_PUBKEY as an administrator: HTTP ${added.status}\n`)
  }

  process.stdout.write(`e2e stack ready at ${baseUrl()} (bunker ${npubFromNsec(process.env.BUNKER_NSEC)})\n`)
  process.stdout.write(`Throwaway administrator, for test use only: ${adminNsec}\n`)
  process.stdout.write('Stop it with: pnpm test:e2e:stack:down\n')
} else if (command === 'down') {
  // compose interpolates the whole file, even to stop it; these values are never used.
  for (const name of ['BUNKER_NSEC', 'E2E_ADMIN_NPUB', 'E2E_PROXY_SECRET', 'E2E_SESSION_PASSWORD']) {
    process.env[name] ??= 'unused'
  }
  down()
} else {
  console.error('Usage: node e2e/stack-cli.ts up|down')
  process.exit(1)
}
