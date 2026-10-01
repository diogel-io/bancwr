// The bunker's health for the navbar indicator and the dashboard card (#28), from #27's status
// response. One shared state and, in the browser, one poll however many indicators are mounted.
import type { BunkerStatus, HealthCheck, HealthState } from '#shared/types/bunker'

/** `unknown` until the first answer, so the indicator never starts green. */
export type IndicatorState = 'unknown' | HealthState

export interface BunkerHealth {
  state: IndicatorState
  checks: HealthCheck[]
  pubkey?: string
  /** Epoch milliseconds of the last answer, or of the last failed attempt. */
  checkedAt?: number
}

export const HEALTH_POLL_MS = 30_000

/** A bunker that is down cannot say so itself: no answer is red, with this as its only check. */
export const NO_ANSWER: HealthCheck = {
  name: 'bunker',
  status: 'fail',
  detail: 'The bunker did not answer. It may be stopped, or the frontend cannot reach it.'
}

export function useBunkerHealth() {
  const health = useState<BunkerHealth>('bunker-health', () => ({ state: 'unknown', checks: [] }))
  const requestFetch = useRequestFetch()

  async function refresh() {
    try {
      const status = await requestFetch<BunkerStatus>('/api/bunker/status')
      health.value = { state: status.status, checks: status.checks ?? [], pubkey: status.pubkey, checkedAt: Date.now() }
    } catch (error) {
      const code = (error as { statusCode?: number, status?: number }).statusCode ?? (error as { status?: number }).status
      // The session or the role, not the bunker's health: the middleware's next navigation deals
      // with it, and showing "Down" to someone whose session expired would be wrong.
      if (code === 401 || code === 403) return
      health.value = { state: 'unhealthy', checks: [NO_ANSWER], pubkey: health.value.pubkey, checkedAt: Date.now() }
    }
  }

  /** Fetches once if nothing has answered yet: during server rendering, or on a client-only mount. */
  async function ensure() {
    if (health.value.state === 'unknown') await refresh()
  }

  return { health, refresh, ensure }
}

// The browser's single poll, shared by every mounted indicator.
let subscribers = 0
let timer: ReturnType<typeof setInterval> | undefined
let poll: (() => Promise<void>) | undefined

function startTimer() {
  stopTimer()
  timer = setInterval(() => poll?.(), HEALTH_POLL_MS)
}

function stopTimer() {
  if (timer) clearInterval(timer)
  timer = undefined
}

// Paused while the tab is hidden; on return, checked at once rather than up to 30 s later.
function onVisibilityChange() {
  if (document.visibilityState === 'hidden') {
    stopTimer()
  } else {
    poll?.()
    startTimer()
  }
}

/** Starts polling while at least one caller is mounted. Call from onMounted; call the result on unmount. */
export function subscribeToBunkerHealth(refresh: () => Promise<void>): () => void {
  poll = refresh
  if (subscribers++ === 0) {
    document.addEventListener('visibilitychange', onVisibilityChange)
    if (document.visibilityState !== 'hidden') startTimer()
  }
  let subscribed = true
  return () => {
    if (!subscribed) return
    subscribed = false
    if (--subscribers === 0) {
      stopTimer()
      document.removeEventListener('visibilitychange', onVisibilityChange)
    }
  }
}
