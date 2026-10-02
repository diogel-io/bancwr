// A NIP-07 extension whose key lives in the test process, as a real extension's would (#11, #30).
import type { Page } from '@playwright/test'
import { finalizeEvent, getPublicKey } from 'nostr-tools/pure'
import { decode } from 'nostr-tools/nip19'

export async function installExtension(page: Page, nsec: string) {
  const key = decode(nsec).data as Uint8Array
  await page.exposeFunction('__extensionSign', (template: Parameters<typeof finalizeEvent>[0]) => finalizeEvent(template, key))
  await page.exposeFunction('__extensionPubkey', () => getPublicKey(key))
  await page.addInitScript(() => {
    const bridge = window as unknown as { __extensionSign: (t: unknown) => unknown, __extensionPubkey: () => Promise<string> }
    ;(window as unknown as { nostr: unknown }).nostr = {
      getPublicKey: () => bridge.__extensionPubkey(),
      signEvent: (template: unknown) => bridge.__extensionSign(template)
    }
  })
}
