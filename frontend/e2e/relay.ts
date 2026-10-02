// A minimal Nostr relay (NIP-01) for the NIP-46 sign-in test, run inside the Playwright process
// so no relay image or public relay is needed. It stores events in memory and supports EVENT, REQ
// and CLOSE with the filter fields NIP-46 uses: ids, kinds, authors, #p, since and limit.
import { WebSocketServer, type WebSocket } from 'ws'

type Filter = { ids?: string[], kinds?: number[], authors?: string[], '#p'?: string[], since?: number, limit?: number }
type Event = { id: string, pubkey: string, kind: number, created_at: number, tags: string[][] }

function matches(event: Event, filter: Filter): boolean {
  if (filter.ids && !filter.ids.includes(event.id)) return false
  if (filter.kinds && !filter.kinds.includes(event.kind)) return false
  if (filter.authors && !filter.authors.includes(event.pubkey)) return false
  if (filter.since && event.created_at < filter.since) return false
  if (filter['#p'] && !event.tags.some(t => t[0] === 'p' && filter['#p']!.includes(t[1]!))) return false
  return true
}

/**
 * `host` is 127.0.0.1 unless the bunker container must reach it (#31: its NIP-46 relay), which
 * needs every interface: from inside the container the host is reached at its network address.
 */
export function startRelay(port: number, host = '127.0.0.1'): Promise<() => Promise<void>> {
  const events: Event[] = []
  const subscriptions = new Map<WebSocket, Map<string, Filter[]>>()
  const server = new WebSocketServer({ port, host })

  server.on('connection', (socket) => {
    subscriptions.set(socket, new Map())
    socket.on('close', () => subscriptions.delete(socket))
    socket.on('message', (data) => {
      let message: unknown[]
      try {
        message = JSON.parse(String(data))
      } catch {
        return
      }
      const [type, ...rest] = message
      if (type === 'EVENT') {
        const event = rest[0] as Event
        events.push(event)
        socket.send(JSON.stringify(['OK', event.id, true, '']))
        for (const [client, subs] of subscriptions) {
          for (const [id, filters] of subs) {
            if (filters.some(f => matches(event, f))) client.send(JSON.stringify(['EVENT', id, event]))
          }
        }
      } else if (type === 'REQ') {
        const [id, ...filters] = rest as [string, ...Filter[]]
        subscriptions.get(socket)!.set(id, filters)
        for (const filter of filters) {
          const found = events.filter(e => matches(e, filter))
          for (const event of found.slice(filter.limit ? -filter.limit : 0)) socket.send(JSON.stringify(['EVENT', id, event]))
        }
        socket.send(JSON.stringify(['EOSE', id]))
      } else if (type === 'CLOSE') {
        subscriptions.get(socket)!.delete(rest[0] as string)
      }
    })
  })

  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.once('listening', () => resolve(() => new Promise<void>((done) => {
      for (const client of server.clients) client.terminate()
      server.close(() => done())
    })))
  })
}
