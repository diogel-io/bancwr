// SPIKE (#23). NIP-07 sign-in in a real Chromium: window.nostr is stubbed, its signEvent backed by
// a key held in this Node process (as an extension would hold it), via page.exposeFunction.
// Usage: U07_NSEC=nsec1… ORIGIN=http://localhost:3300 node spike/nip07-sign-in.mts
import { chromium } from '@playwright/test'
import { finalizeEvent, getPublicKey } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'

const key = decode(process.env.U07_NSEC!).data as Uint8Array
const origin = process.env.ORIGIN!
const browser = await chromium.launch()
const context = await browser.newContext()
const page = await context.newPage()
const prompts: number[] = []
await page.exposeFunction('__extensionSign', (template: any) => { prompts.push(template.kind); return finalizeEvent(template, key) })
await page.addInitScript(() => {
  (window as any).nostr = {
    getPublicKey: async () => { throw new Error('not used') },
    signEvent: (template: unknown) => (window as any).__extensionSign(template)
  }
})
const out = page.getByTestId('output')
const step = async (button: string) => {
  const before = await out.innerText()
  await page.getByRole('button', { name: button }).click()
  await page.waitForFunction(b => document.querySelector('[data-testid=output]')?.textContent !== b, before)
  return out.innerText()
}

await page.goto(`${origin}/spike-sign-in`)
process.stdout.write(`sign in    ${await step('Sign in with extension')}\n`)
process.stdout.write(`signer asked to sign kinds: ${JSON.stringify(prompts)}\n`)
const cookie = (await context.cookies()).find(c => c.name === 'bancwr-session')
process.stdout.write(`cookie     httpOnly=${cookie?.httpOnly} secure=${cookie?.secure} sameSite=${cookie?.sameSite} expires_in_h=${cookie ? ((cookie.expires - Date.now() / 1000) / 3600).toFixed(1) : '-'}\n`)
process.stdout.write(`page JS can read it: ${await page.evaluate(() => document.cookie.includes('bancwr-session'))}\n`)
process.stdout.write(`whoami     ${await step('Who am I (via proxy)')}  expected ${getPublicKey(key).slice(0, 12)}…\n`)
process.stdout.write(`sign out   ${await step('Sign out')}\n`)
process.stdout.write(`whoami     ${await step('Who am I (via proxy)')}\n`)
await browser.close()
