// Relay reads and writes for the member's own events (#30), behind a small interface so the
// profile logic can be tested without sockets. Client-only (README, "Nostr (nostr-tools)").
import { SimplePool } from 'nostr-tools/pool'
import { verifyEvent, type NostrEvent } from 'nostr-tools/pure'
import type { Filter } from 'nostr-tools/filter'

export interface QueryResult {
  events: NostrEvent[]
  /** Relays that connected and answered. */
  reached: string[]
  failed: string[]
}

export interface PublishResult {
  accepted: string[]
  failed: { url: string, reason: string }[]
}

export interface RelayIO {
  query(relays: string[], filter: Filter): Promise<QueryResult>
  publish(relays: string[], event: NostrEvent): Promise<PublishResult>
}

export const CONNECT_TIMEOUT_MS = 4000
export const QUERY_WAIT_MS = 5000

export function createRelayIO(): RelayIO {
  const pool = new SimplePool()
  return {
    async query(relays, filter) {
      // Connect first, relay by relay, so a save can tell "no relay answered" from "no profile".
      const attempts = await Promise.allSettled(relays.map(url => pool.ensureRelay(url, { connectionTimeout: CONNECT_TIMEOUT_MS })))
      const reached = relays.filter((_, i) => attempts[i]!.status === 'fulfilled')
      const failed = relays.filter((_, i) => attempts[i]!.status === 'rejected')
      const events = reached.length ? await pool.querySync(reached, filter, { maxWait: QUERY_WAIT_MS }) : []
      // The pool verifies too; checked again here as the events arrive, never after editing.
      return { events: events.filter(event => verifyEvent(event)), reached, failed }
    },
    async publish(relays, event) {
      const outcomes = await Promise.allSettled(pool.publish(relays, event))
      return {
        accepted: relays.filter((_, i) => outcomes[i]!.status === 'fulfilled'),
        failed: relays.flatMap((url, i) => {
          const outcome = outcomes[i]!
          return outcome.status === 'rejected' ? [{ url, reason: outcome.reason instanceof Error ? outcome.reason.message : String(outcome.reason) }] : []
        })
      }
    }
  }
}
