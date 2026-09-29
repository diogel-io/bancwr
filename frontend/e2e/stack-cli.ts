// `pnpm test:e2e:stack` / `pnpm test:e2e:stack:down`: runs the e2e stack on its own, for driving
// it by hand or through the Playwright MCP server in ../../.mcp.json. Node runs this file directly
// (type stripping), hence the .ts import specifiers.
import { generateNsec, npubFromNsec } from './keys.ts'
import { baseUrl, down, up, waitForBunker } from './stack.ts'

const command = process.argv[2]

if (command === 'up') {
  process.env.BUNKER_NSEC = generateNsec()
  up()
  await waitForBunker()
  process.stdout.write(`e2e stack ready at ${baseUrl()} (bunker ${npubFromNsec(process.env.BUNKER_NSEC)})\n`)
  process.stdout.write('Stop it with: pnpm test:e2e:stack:down\n')
} else if (command === 'down') {
  // compose interpolates the whole file, even to stop it; the value is never used.
  process.env.BUNKER_NSEC ??= 'unused'
  down()
} else {
  console.error('Usage: node e2e/stack-cli.ts up|down')
  process.exit(1)
}
