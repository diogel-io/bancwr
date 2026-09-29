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
proxy to the bunker. Vitest's `tests/` covers components in isolation; this covers the pages,
navigation, adding and removing team members, and the Config page staying read-only and never
showing a key.

Run these commands from this `frontend/` directory, or from the repository root with
`pnpm -C frontend`, for example `pnpm -C frontend test:e2e`: the root has no `package.json`.

Each run:

1. generates a throwaway bunker key, held only in the environment;
2. builds and starts the stack on <http://localhost:3100>, with the database on a tmpfs, so every
   run starts empty and leaves nothing on disk. The bunker is not published on any host port;
3. waits until `GET /api/bunker/status` answers `healthy` through the proxy;
4. runs the specs in Chromium, one at a time;
5. saves the container logs to `test-results/compose.log` and removes the stack.

Prerequisites: Docker Compose or Podman Compose, and Chromium for Playwright, installed once:

```bash
pnpm exec playwright install chromium
```

| Variable | Default | Use |
|----------|---------|-----|
| `E2E_COMPOSE` | the first of `docker compose`, `podman compose`, `docker-compose` that works | Compose command |
| `E2E_PORT` | `3100` | Host port for the frontend |
| `E2E_BUILD` | build | `0` reuses images already tagged `bancwr-backend:e2e` and `bancwr-frontend:e2e`, as CI does |
| `E2E_BASE_URL` | unset | Test a stack that is already running instead of starting one. Pass its `BUNKER_NSEC` too, or the public-key check is skipped |
| `E2E_READY_TIMEOUT` | `120000` | Milliseconds to wait for the bunker |

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
pnpm test:e2e:stack        # start, with a fresh key and empty database; prints the bunker npub
pnpm test:e2e:stack:down   # stop and remove it
```

Keep the version in that command in step with `.mcp.json`.

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

**Relay I/O is client-only.** The production image runs Node 20 (`node:20-slim` in the
`Dockerfile`), which has no global `WebSocket`, so `SimplePool` on the server fails with
`ReferenceError: WebSocket is not defined`. Local development runs a newer Node that has one, so
the same code works under `pnpm dev` and breaks only once deployed. Open relays from `onMounted`, a
`.client` plugin, inside `<ClientOnly>`, or behind `import.meta.client`, and never from a
server-side `useAsyncData` or a route in `server/`. If the image moves to Node 22 or later (#16),
this stops being a hard failure, but relays still belong in the browser: the server would otherwise
hold relay connections on behalf of every visitor.

### Rules

- **Import from the subpaths** (`nostr-tools/pure`, `nostr-tools/nip19`, `nostr-tools/pool`), never
  from `nostr-tools` itself. The package root is a separate bundle with its own copy of every
  module, so mixing the two ships the code twice.
- **Never re-verify an edited copy of a verified event.** `verifyEvent` and `finalizeEvent` cache
  `true` on the event object under a symbol, and object spread copies it: `{ ...event, content }`
  still passes `verifyEvent` after its content changes. Verify events as they arrive (parsed from
  JSON), and sign a new event with `finalizeEvent` rather than editing a signed one.
- The bunker's own key never reaches the frontend. A signed-in user's events are signed through
  NIP-07 or NIP-46 with their key; see #30.

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
