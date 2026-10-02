# Bancwr Frontend

The Nuxt web interface for the Bancwr bunker. It serves the dashboard and proxies bunker API
calls to the Rust backend, so the backend does not need to be exposed to the browser.

Package manager is pnpm, pinned in `package.json`. npm, yarn and bun are not supported here.

## Architecture

| Part | Address | Source |
|------|---------|--------|
| Frontend (Nuxt) | <http://localhost:3001> | this directory |
| Backend (Rust) | <http://localhost:3000> | [`../backend`](../backend) |

Requests to `/api/bunker/*` are proxied to the backend by `server/api/bunker/[...].ts`. The
target comes from `NUXT_API_BASE` and defaults to `http://localhost:3000`. Nothing else is
proxied. The proxy refuses a target that resolves to this server, rather than looping.

Which role may open which page is set in one place, `app/utils/access.ts`: the sidebar offers only
those pages, and the route middleware refuses the rest with a permission-denied page. Users and
signers see the dashboard's bunker health only. A new page adds its route there. This is for
usability: the bunker enforces the same matrix itself (see [`../backend`](../backend/README.md)).

Every page's header is `AppNavbar`, which carries the bunker's health top left (`BunkerHealth`):
green, yellow or red with a text label, and each check's detail on click. It polls
`/api/bunker/status` every 30 seconds while the tab is visible, through one shared state
(`useBunkerHealth`); no answer counts as red. The checks themselves are the bunker's (see
[`../backend`](../backend/README.md#bunker-status)).

## Setup

```bash
pnpm install
```

## Running

The dashboard needs the backend running to show anything. Either start both from here:

```bash
pnpm dev:all
```

Or run them in separate terminals, which gives clearer logs:

```bash
# Terminal 1
cd ../backend && cargo run

# Terminal 2
pnpm dev
```

Then open <http://localhost:3001>. The dev server port is set by `devServer.port` in
`nuxt.config.ts`.

Sign-in (#11) needs three settings, or the server refuses to start. For local development, put
them in `frontend/.env` (ignored by git), and give the backend the same `BANCWR_PROXY_SECRET` and
your npub as `BANCWR_ADMIN_PUBKEY`:

```bash
NUXT_SESSION_PASSWORD=<openssl rand -hex 32>
NUXT_PROXY_SECRET=<openssl rand -hex 32, the backend's BANCWR_PROXY_SECRET>
NUXT_SITE_ORIGIN=http://localhost:3001
```

Plain `http://` is accepted only for localhost; anywhere else Bancwr must be served over HTTPS (see
the root README). Sign in with a NIP-07 extension or a NIP-46 `bunker://` string.

To point the frontend at a backend somewhere other than `localhost:3000`:

```bash
NUXT_API_BASE=http://bunker.example:3000 pnpm dev
```

## Checks

```bash
pnpm lint        # ESLint; pnpm lint:fix to apply fixes
pnpm typecheck   # vue-tsc
pnpm test        # Vitest
```

## End-to-end tests

`pnpm test:e2e` runs the Playwright suite in `e2e/` against the real stack: the backend and
frontend images, started from `e2e/compose.e2e.yaml`, with the browser going through the frontend
proxy to the bunker. Vitest's `tests/` covers components in isolation; this covers sign-in
(NIP-07 and NIP-46), the pages, navigation, adding and removing team members, and the Config page
staying read-only and never showing a key.

Run these commands from this `frontend/` directory, or from the repository root with
`pnpm -C frontend`, for example `pnpm -C frontend test:e2e`: the root has no `package.json`.

Each run:

1. generates throwaway keys and secrets, held only in the environment: the bunker's key, a seeded
   administrator's key, the proxy secret and the session password;
2. builds and starts the stack on <http://localhost:3100>, with the database on a tmpfs, so every
   run starts empty and leaves nothing on disk. The bunker is not published on any host port;
3. waits until the administrator can sign in and see the bunker `healthy` through the proxy, which
   proves sign-in, the session, the signed proxy and the bunker's guard;
4. starts a minimal relay (`e2e/relay.ts`, on `127.0.0.1:7777`) and a NIP-46 test signer
   (`e2e/remote-signer.ts`) inside the Playwright process, and registers that signer's key as a
   `user`, then registers one more key as a `user` and one as a `signer`, for the role specs;
5. runs the specs in Chromium, one at a time. `test` from `e2e/fixtures.ts` starts each one signed
   in as the administrator, or as another role with `test.use({ role: 'signer' })`;
   `anonymousTest` does not sign in;
6. saves the container logs to `test-results/compose.log` and removes the stack.

Prerequisites: Docker Compose or Podman Compose, and Chromium for Playwright, installed once:

```bash
pnpm exec playwright install chromium
```

| Variable | Default | Use |
|----------|---------|-----|
| `E2E_COMPOSE` | the first of `docker compose`, `podman compose`, `docker-compose` that works | Compose command |
| `E2E_PORT` | `3100` | Host port for the frontend |
| `E2E_BUILD` | build | `0` reuses images already tagged `bancwr-backend:e2e` and `bancwr-frontend:e2e`, as CI does |
| `E2E_BASE_URL` | unset | Test a stack that is already running instead of starting one. Pass `E2E_ADMIN_NSEC` (an administrator of that stack), and its `BUNKER_NSEC` too, or the public-key check is skipped |
| `E2E_READY_TIMEOUT` | `120000` | Milliseconds to wait for the stack |
| `E2E_RELAY_PORT` | `7777` | Port for the in-process relay the NIP-46 spec uses |

A failed test keeps its trace. Open the HTML report with `pnpm exec playwright show-report`, or
a single trace with `pnpm exec playwright show-trace test-results/<test>/trace.zip`.
`pnpm test:e2e:ui` runs the suite in Playwright's UI mode.

In CI, the `e2e` job in `.github/workflows/ci.yml` runs the suite on pull requests and pushes to
`master`. It is not a required check yet. When it fails, the report, traces and container logs
are uploaded as the `playwright-report` artifact.

### Driving the app with Playwright MCP

`.mcp.json` at the repository root registers the [Playwright MCP](https://playwright.dev/mcp/installation)
server, so an agent such as Claude Code can drive a browser while writing or debugging tests.
The MCP server bundles its own Playwright, so it needs its own Chromium build, installed once:

```bash
npx @playwright/mcp@0.0.83 install-browser chromium
```

Start the stack on its own, and point the agent at <http://localhost:3100>:

```bash
pnpm test:e2e:stack        # start, with fresh keys and an empty database
pnpm test:e2e:stack:down   # stop and remove it
```

Sign-in is required. The stack seeds a throwaway administrator and prints its nsec, for test use
only. To sign in with your own extension instead, pass your npub, and it is registered as an
administrator too: `E2E_ADMIN_PUBKEY=npub1… pnpm test:e2e:stack`.

Keep the version in that command in step with `.mcp.json`.

## Pinned dependencies

`package.json` cannot hold comments, so pins and their reasons are listed here.

| Package | Pinned to | Why | Unpin when |
|---------|-----------|-----|------------|
| `vue` (dependency and `pnpm.overrides`) | `3.5.38` | From 3.5.39 to at least 3.5.43, a page rendered on the server and hydrated in the browser keeps its `UFormField` labels pointing at the server's `useId()` values while the inputs are re-rendered with new ones, so `<label for>` no longer matches any `id`. Labels stop naming their fields for screen readers, and `getByLabel` finds nothing. Found by `e2e/team.spec.ts`, bisected in #16. (3.5.36 cannot be installed at all: it was published with `workspace:*` dependencies.) The override keeps Nuxt from installing a second, newer Vue. | A newer Vue passes `pnpm test:e2e` |

## Nostr (`nostr-tools`)

[`nostr-tools`](https://github.com/nbd-wtf/nostr-tools) is the frontend's Nostr library, for
profile, follow, relay and connection work (#30–#33). It is pinned to an exact version in
`package.json`, and Renovate proposes upgrades as pull requests. `tests/nostr/` holds a smoke test
that runs the same checks in the Nuxt environment and in plain Node.

### Where each part may run

| Part | Server (SSR, Nitro routes) | Client |
|------|---------------------------|--------|
| `nostr-tools/nip19`, `nostr-tools/pure` (`getPublicKey`, `finalizeEvent`, `verifyEvent`) | yes | yes |
| Relay I/O: `nostr-tools/pool` (`SimplePool`), `nostr-tools/relay` | **no** | yes |
| NIP-07 signing (`window.nostr`) | no | yes |

**Relay I/O is client-only.** Open relays from `onMounted`, a `.client` plugin, inside
`<ClientOnly>`, or behind `import.meta.client`, and never from a server-side `useAsyncData` or a
route in `server/`: the server would otherwise hold relay connections on behalf of every visitor,
and could be made to connect wherever a request pointed it. The image now runs Node 24 (#16), which
has a global `WebSocket`, so a server-side `SimplePool` would no longer fail outright as it did on
Node 20; the rule stands for those reasons, not for a missing API.

### Rules

- **Import from the subpaths** (`nostr-tools/pure`, `nostr-tools/nip19`, `nostr-tools/pool`), never
  from `nostr-tools` itself. The package root is a separate bundle with its own copy of every
  module, so mixing the two ships the code twice.
- **Never re-verify an edited copy of a verified event.** `verifyEvent` and `finalizeEvent` cache
  `true` on the event object under a symbol, and object spread copies it: `{ ...event, content }`
  still passes `verifyEvent` after its content changes. Verify events as they arrive (parsed from
  JSON), and sign a new event with `finalizeEvent` rather than editing a signed one.
  `finalizeEvent` also flags **the template it is given**, so never reuse a template after signing
  it. A signer's answer is checked as plain JSON for the same reason (`checkedSigner`).
- The bunker's own key never reaches the frontend. A signed-in user's events are signed through
  NIP-07 or NIP-46 with their key: `useUserSigner()` (below).

### Profile

`/profile` (#30) edits the signed-in member's own kind 0 profile, for every role. Everything runs in
the browser; the bunker is not involved.

- **Signing.** Sign-in records, for the tab, whether the member used NIP-07 or NIP-46.
  `useUserSigner().signer()` returns their extension, if it holds the signed-in key, or resumes the
  tab's NIP-46 connection. Without one (a new tab after a NIP-46 sign-in, or an extension on another
  key) the page asks them to reconnect with `SignerConnect`, which refuses any other key. Every
  signed event is checked to be from the signed-in key before it is published.
- **Relays.** The member's NIP-65 write relays (kind 10002), plus `NUXT_PUBLIC_PROFILE_RELAYS`
  (comma-separated; empty means damus, nos.lol and primal), plus indexer relays,
  `NUXT_PUBLIC_INDEXER_RELAYS` (empty means purplepag.es, profiles.nostr1.com and relay.nos.social),
  read and published to alike. The member's relay list is looked up on the defaults and the
  indexers: indexers collect profiles and relay lists from across the network, so a member whose
  list and profile are on none of the other relays is still found (#62).
- **Saving never drops fields.** Save reads the newest kind 0 again, changes only the form's
  fields, and keeps every other key as it was. If no relay answers that read, it refuses.
- **No profile found** is not taken as "no profile" (#62). The page lists the relays it searched,
  offers to search another for this visit, and creates a profile only once the member confirms
  they have none elsewhere: a new profile would replace one held on a relay not searched.
- **Images** upload to the member's Blossom server (kind 10063), or `NUXT_PUBLIC_BLOSSOM_SERVER`
  (empty means blossom.primal.net), authorised by a kind 24242 event their signer signs. Stills are
  re-encoded as WebP, which drops camera and location metadata; GIFs are sent as they are.
  Uploading fills the field; nothing is published until Save.
- **NIP-05** is verified only when the member asks, from the browser, and a result for a value they
  have since changed is dropped.

### Follows

`/follows` (#32) edits the signed-in member's own follow list (NIP-02 kind 3, read at nips commit
`0046368a`), for administrators and users. It uses the same relays and signer as the profile page
(`useMemberRelays`, `useSignerPrompt`). Adds (by npub, hex or NIP-05) and removes are staged and
published together with **Save changes**.

Kind 3 is replaceable, so a publish replaces the whole list. Save is guarded so that nothing the
member did not remove can be lost:

1. It reads the newest list again, and refuses if no relay answers, or if what it gets is older
   than the list it loaded (a relay that held the newer list did not answer: an incomplete read).
2. It applies the staged changes to that list, so a follow made in another app since loading is
   kept.
3. It keeps every other tag (`t`, `a`, anything), each `p` tag's relay hint and petname, and the
   content (some clients keep a relay map there), and checks that the result differs by exactly
   the staged changes before anything is signed.

When no list is found anywhere, the page lists the relays searched, offers to search another, and
starts a new list only once the member confirms they have none elsewhere.

### Relays

`/relays` (#33) edits the signed-in member's own NIP-65 relay list (kind 10002, read at nips commit
`0046368a`), for administrators and users: their **write** relays, where other apps read their
posts, and **read** relays, where apps look for mentions of them. No marker means both. It is the
list `/profile` and `/follows` read through, so they use the new list straight after a save.

- URLs follow Porwr's rules (`app/utils/relay-url.ts`): `ws://` or `wss://`, no fragment, a real
  hostname, at most 255 characters, host lowercased, a trailing slash on an empty path dropped.
  `ws://` to anything but this machine is flagged as unencrypted.
- Saving carries the follow list's guards: re-read, refuse when no relay answers or the re-read is
  older, apply the staged changes to it, keep every untouched tag and the content, and check the
  result before signing. A relay with neither Read nor Write is removed.
- The new list is published to the old list's relays, the new list's, the defaults and the
  indexers, as NIP-65 asks, so relays dropped from the list do not keep the old one.
- NIP-65's advice (2 to 4 of each) and a missing write relay are shown, not enforced. With no list,
  the page says which relays Bancwr uses instead and offers to start from the defaults.
- The bunker's own NIP-46 relays (`NIP46_RELAYS`, from `/api/bunker/status`) are shown for
  context: they are operator settings, and unrelated to the member's list, since the bunker signs
  only as itself.

### Differences from Porwr

Porwr resolves `nostr-tools` 2.23.5; Bancwr pins 2.25.2. Changes between the two that matter when
matching Porwr's behaviour:

- `SimplePool.publish()` rejects on a connection failure, where it used to resolve.
- Relay connections left unused are closed automatically after a while.
- `subscribeMap`'s `onClose` receives `{ url, reason }[]` instead of `reason[]`.
- NIP-46 requires a secret in `nostrconnect://` URIs.

## Production

```bash
pnpm build       # build to .output
pnpm preview     # run the built output locally
```

The container build runs `pnpm build` and serves `.output/server/index.mjs`; see `Dockerfile`.
For running the full stack from published images, see the [root README](../README.md).
