# Spike: Nostr-key sessions (diogel-io/bancwr#23)

**Not for merging.** Evidence for the decision record
`diogel-io/workspace:10-products/bancwr/architecture/nostr-session-auth-adr.md`. #11 and #25
build the real thing from that record; this branch shows it works.

## What is here

| Path | What |
|------|------|
| `server/utils/auth/login-event.ts` | The verification rules, numbered as in the ADR |
| `server/utils/auth/challenges.ts` | Single-use challenges, in memory |
| `server/utils/auth/session.ts` | h3 `useSession`: sealed, `HttpOnly; Secure; SameSite=Strict`, 12 h |
| `server/utils/auth/proxy-signature.ts` | The HMAC the proxy attaches for the bunker |
| `server/api/auth/{challenge,login,logout,session}` | The routes |
| `server/api/bunker/[...].ts` | The proxy, requiring a session and signing identity when `NUXT_PROXY_SECRET` is set |
| `app/utils/nostr-sign-in.ts` | Browser sign-in, for any signer with `signEvent` (NIP-07 or NIP-46) |
| `app/pages/spike-sign-in.vue` | A bare page to drive NIP-07 in a real browser |
| `tests/server/auth/*` | One test per rule; the HMAC vector shared with the backend |
| `../backend/src/proxy_auth.rs`, `../backend/tests/unit/proxy_auth_test.rs` | The bunker's check of that HMAC, on `/api/bunker/whoami` |
| `test-remote-signer.mjs` | A minimal NIP-46 remote signer (test use only) |
| `nip46-sign-in.mts`, `nip07-sign-in.mts` | The two end-to-end runs |

`registry.ts` is a stub (`BANCWR_SPIKE_MEMBERS`); the real lookup is #24's.

## Reproducing the end-to-end runs

With throwaway keys only. From `frontend/`, after `pnpm build` and
`podman build -t localhost/bancwr-backend:23spike ../backend`:

```bash
podman network create auth23
podman run -d --rm --name relay23 --network auth23 --network-alias relay -p 127.0.0.1:7777:8080 ghcr.io/nostrfi/relay:latest
podman run -d --rm --name app-bunker --network auth23 -e BUNKER_NSEC=$APP_NSEC -e BANCWR_PROXY_SECRET=$PROXY \
  -e DATABASE_PATH=/data/b.db --tmpfs /data -p 127.0.0.1:3301:3000 localhost/bancwr-backend:23spike
SIGNER_NSEC=$U46_NSEC RELAY=ws://127.0.0.1:7777 node spike/test-remote-signer.mjs &
PORT=3300 NUXT_API_BASE=http://127.0.0.1:3301 NUXT_SITE_ORIGIN=http://localhost:3300 \
  NUXT_SESSION_PASSWORD=$SESSION NUXT_PROXY_SECRET=$PROXY \
  BANCWR_SPIKE_MEMBERS="$U07_HEX:administrator,$U46_HEX:user" node .output/server/index.mjs &

SIGNER_HEX=$U46_HEX RELAY=ws://127.0.0.1:7777 ORIGIN=http://localhost:3300 node spike/nip46-sign-in.mts
U07_NSEC=$U07_NSEC ORIGIN=http://localhost:3300 node spike/nip07-sign-in.mts
```

Results from 2026-09-29 are recorded in the ADR.
