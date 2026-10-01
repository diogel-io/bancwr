import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { CONNECT_TIMEOUT_MS, Nip46Cancelled, Nip46Timeout, SIGN_TIMEOUT_MS, useNip46 } from '~/composables/useNip46'

const { signer, fromBunker } = vi.hoisted(() => {
  const signer = {
    connect: vi.fn(),
    signEvent: vi.fn(),
    logout: vi.fn(),
    close: vi.fn()
  }
  return { signer, fromBunker: vi.fn(() => signer) }
})
vi.mock('nostr-tools/nip46', () => ({ BunkerSigner: { fromBunker } }))

const URI = `bunker://${'ab'.repeat(32)}?relay=wss://relay.example`
const never = () => new Promise(() => {})

describe('useNip46', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    sessionStorage.clear()
    for (const fn of Object.values(signer)) fn.mockReset()
    signer.close.mockResolvedValue(undefined)
    signer.logout.mockResolvedValue(undefined)
    fromBunker.mockClear()
  })
  afterEach(() => vi.useRealTimers())

  it('refuses anything but a bunker:// string, without contacting anyone', async () => {
    await expect(useNip46().connect('alice@example.com')).rejects.toThrow('bunker://')
    expect(fromBunker).not.toHaveBeenCalled()
  })

  it('times out a signer that never answers connect, with a message', async () => {
    signer.connect.mockImplementation(never)
    const attempt = useNip46().connect(URI)
    const expectation = expect(attempt).rejects.toBeInstanceOf(Nip46Timeout)
    await vi.advanceTimersByTimeAsync(CONNECT_TIMEOUT_MS)
    await expectation
    await expect(attempt).rejects.toThrow('did not answer')
  })

  it('times out signing inside the 120 s window', async () => {
    signer.connect.mockResolvedValue(undefined)
    signer.signEvent.mockImplementation(never)
    const remote = await useNip46().connect(URI)
    const attempt = remote.signEvent({ kind: 27235, created_at: 0, tags: [], content: '' })
    const expectation = expect(attempt).rejects.toBeInstanceOf(Nip46Timeout)
    await vi.advanceTimersByTimeAsync(SIGN_TIMEOUT_MS)
    await expectation
    expect(SIGN_TIMEOUT_MS).toBeLessThan(120_000)
  })

  it('can be cancelled while waiting, and forgets the client key', async () => {
    signer.connect.mockImplementation(never)
    const nip46 = useNip46()
    const attempt = nip46.connect(URI)
    const expectation = expect(attempt).rejects.toBeInstanceOf(Nip46Cancelled)
    await vi.advanceTimersByTimeAsync(10)
    expect(sessionStorage.getItem('bancwr-nip46')).not.toBeNull()
    await nip46.cancel()
    await expectation
    expect(signer.close).toHaveBeenCalled()
    expect(sessionStorage.getItem('bancwr-nip46')).toBeNull()
  })

  it('shows, but never opens, the signer\'s approval URL', async () => {
    const open = vi.spyOn(window, 'open')
    signer.connect.mockImplementation(never)
    const nip46 = useNip46()
    nip46.connect(URI).catch(() => {})
    // connect() first closes any earlier connection, so the signer is created a tick later.
    await vi.advanceTimersByTimeAsync(0)
    const params = (fromBunker.mock.calls[0] as unknown[])[2] as { onauth: (url: string) => void }
    params.onauth('https://signer.example/approve/1')
    expect(nip46.approvalUrl.value).toBe('https://signer.example/approve/1')
    expect(nip46.phase.value).toBe('awaiting-approval')
    expect(open).not.toHaveBeenCalled()
    await nip46.cancel()
  })

  it('on sign-out, tells the signer and deletes the client key', async () => {
    signer.connect.mockResolvedValue(undefined)
    const nip46 = useNip46()
    await nip46.connect(URI)
    await nip46.forget()
    expect(signer.logout).toHaveBeenCalled()
    expect(sessionStorage.getItem('bancwr-nip46')).toBeNull()
  })
})
