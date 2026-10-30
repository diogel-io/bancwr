# Bancwr Threat Model

Status: current

Baseline: `diogel-io/bancwr` `master` at `b44bcaf`

This document covers what Bancwr protects, where its trust boundaries are, what an attacker at
each boundary can do, what stops them, and what does not yet. To report a vulnerability, see
[`SECURITY.md`](../SECURITY.md).

## How to read and maintain this document

- Assets are `A`, boundaries `B`, adversaries `X`, and threats `T`. The IDs are stable: a row that
  no longer applies is marked as such, never renumbered.
- Each control names the code that enforces it and, where one exists, the test that holds it
  (section 7). A control with no test says so.
- **Any change that adds or changes an asset, a trust boundary, or a control updates this file in
  the same pull request**, and moves the baseline above if it re-checks the whole document.
- Only limits that are already public, in the READMEs or open issues, are written here. A newly
  found weakness is reported privately under [`SECURITY.md`](../SECURITY.md) and fixed first, and
  added here afterwards.

The design behind sign-in and the proxy is the sign-in decision record,
`diogel-io/workspace` `10-products/bancwr/architecture/nostr-session-auth-adr.md` (#23). Where it
and this document differ about current behaviour, this document and the code win.

## Scope and method

In scope: Bancwr as `compose.yaml` ships it. One bunker container (Rust, `backend/`) and one
frontend container (Nuxt and Nitro, `frontend/`) on one host, joined by the compose network. A
TLS reverse proxy run by the operator sits in front of the frontend. The bunker holds one signing
key, signs NIP-46 requests that arrive over Nostr relays, and serves an administration API that
only the frontend calls.

Out of scope: running several frontend instances, hosted Bancwr, and the operator's reverse proxy
and host hardening beyond what Bancwr requires of them. An independent review is a separate task
(diogel-io/workspace#175).

Method: every row was written by reading the code named on `master` at the baseline commit, and
the READMEs. Nothing here was penetration-tested.

## 1. Assets

| ID | Asset | Where it lives | Why it matters |
|----|-------|----------------|----------------|
| A1 | The bunker's private key | `BUNKER_NSEC`, which is what `compose.yaml` passes; outside compose the bunker can read it instead from the file `BUNKER_NSEC_FILE` names. Read at start (`backend/src/config.rs`). Held in bunker memory (`backend/src/signer.rs`). Not encrypted at rest | Whoever has it is that Nostr identity, permanently: Nostr has no key rotation or revocation |
| A2 | The ability to make the bunker sign | The NIP-46 handler (`backend/src/nip46.rs`) | An attacker does not need A1 if they can get events signed with it |
| A3 | The proxy secret | `BANCWR_PROXY_SECRET` on the bunker, `NUXT_PROXY_SECRET` on the frontend, the same value | With it, anyone who reaches the bunker can call its API as any identity |
| A4 | The session-sealing password | `NUXT_SESSION_PASSWORD` on the frontend | Seals the session cookie |
| A5 | Live sessions and login challenges | Frontend process memory (`frontend/server/utils/auth/sessions.ts`, `challenges.ts`) | A live session id is a signed-in user |
| A6 | The member registry and roles | SQLite at `DATABASE_PATH` (`backend/src/registry.rs`, `backend/src/db.rs`) | Decides who may do what |
| A7 | Connection tokens and connections | SQLite. Tokens are stored as a SHA-256 hash only (`nip46.rs`, `hash_secret`) | A connection may sign the kinds it was granted |
| A8 | The signing log and configuration | SQLite: the audit log of signatures, the bunker relays | Accountability, and where NIP-46 traffic goes |
| A9 | The first-administrator setting | `BANCWR_ADMIN_PUBKEY` | Seeds the first administrator when there is none |

## 2. Trust boundaries

| ID | Boundary | Crossed by | Enforced in |
|----|----------|------------|-------------|
| B1 | Browser ↔ operator's TLS reverse proxy | Every request to the console | The operator's proxy. The frontend refuses to start without an `https://` `NUXT_SITE_ORIGIN`, except on localhost (`frontend/server/utils/auth/config.ts`) |
| B2 | Browser ↔ Nitro frontend | Sign-in, the session cookie, every console call | `frontend/server/api/auth/*`, `frontend/server/utils/auth/*` |
| B3 | Nitro proxy ↔ bunker API | Every `/api/bunker/*` call, signed with A3 | `frontend/server/api/bunker/[...].ts`, `frontend/server/utils/auth/proxy-signature.ts`, `backend/src/proxy_auth.rs` |
| B4 | Bunker ↔ relays ↔ NIP-46 apps | NIP-46 requests and responses, kind 24133 | `backend/src/relay.rs`, `backend/src/nip46.rs`, `backend/src/bunker_relays.rs` |
| B5 | A member's own signer ↔ the browser | The sign-in event only (NIP-07 extension or NIP-46 remote signer) | The member's signer; Bancwr never sees the member's key |
| B6 | Containers ↔ host and operator | Environment, files, `./data`, images pulled from GHCR | The host; non-root distroless images (#16) |

The bunker's port is not published by `compose.yaml`: only the frontend container reaches it, over
the compose network. The frontend is published on `127.0.0.1:3001` only, for the reverse proxy.

## 3. Adversaries

| ID | Adversary | Starts with |
|----|-----------|-------------|
| X1 | Network attacker | Can watch or alter traffic between the browser and the host |
| X2 | Unauthenticated web visitor | Can reach the console's sign-in page; may run a malicious site the member visits |
| X3 | Registered member beyond their role | A real session as a viewer or signer |
| X4 | NIP-46 app | A connection token, or a live connection, for some kinds |
| X5 | Malicious or compromised relay | Sees and can drop, delay or replay NIP-46 traffic through it |
| X6 | Direct caller to the bunker | Can reach the bunker's port, without A3 |
| X7 | Holder of a copied session cookie | The cookie, without the browser it came from |
| X8 | Host or operator access | The environment, files and containers |
| X9 | Supply-chain attacker | A dependency, base image or published image |

## 4. Threats and controls

| ID | Threat | Boundary | Adversary | Controls |
|----|--------|----------|-----------|----------|
| T1 | Sign in as someone else by forging or replaying a sign-in event | B2 | X1, X2 | Sign-in is a NIP-98 event (kind 27235) bound to the login URL taken from `NUXT_SITE_ORIGIN`, never from `Host`; to `POST`; and through the `payload` tag to a body carrying a server challenge. The challenge is single-use and lives 300 s: once the body parses and names a string challenge, it is consumed before the event is checked, whether or not the rest passes. A body that does not parse, or names no string challenge, is refused without consuming anything. `created_at` must be within 120 s past and 30 s future. The signature is verified on a freshly parsed object (`frontend/server/utils/auth/login-event.ts`, `challenges.ts`, `constants.ts`) |
| T2 | Sign in with the bunker's own key, so that a NIP-46 app able to sign a login event becomes an administrator | B2, B4 | X4 | The bunker's key is refused at sign-in (403 `bunker_key`), can never be registered, and is refused as `BANCWR_ADMIN_PUBKEY` at start (`login-event.ts`, `backend/src/server.rs`, `backend/src/config.rs`). See "Why not the bunker's key?" in the README |
| T3 | Steal or reuse a session | B1, B2 | X1, X2, X7 | The cookie is `HttpOnly; Secure; SameSite=Strict; Path=/`, 12 h, sealed with A4, and holds only a random 32-byte session id (`frontend/server/utils/auth/session.ts`). Signing out or signing in again revokes the id server-side, so a copied cookie stops working (`sessions.ts`). HTTPS is required (B1), except plain HTTP to `localhost`, `127.0.0.1` or `[::1]` |
| T4 | Keep access after removal or demotion | B2, B3 | X3 | The role is read from the registry on every request, never stored in the session (`frontend/server/api/auth/session.get.ts`, `backend/src/proxy_auth.rs`). Removing a member, or making them a viewer, revokes their NIP-46 connections and tokens (`member_removed`, `role_changed`) |
| T5 | Do more than the role allows | B3 | X3 | The bunker applies one route-by-role matrix to the verified identity (`proxy_auth.rs`, `Access`; table in `backend/README.md`). The console's page gating is cosmetic; the bunker enforces. The last administrator cannot be removed (`backend/src/server.rs`) |
| T6 | Call the bunker's API directly, or claim an identity to it | B3 | X6 | Every `/api/bunker/*` route needs `x-bancwr-identity`, `x-bancwr-timestamp` and `x-bancwr-signature`: HMAC-SHA256 under A3 of `v1`, the timestamp, the method, the path with query, and the identity. It is compared in constant time with a 30 s window. A failed signature check is 401; an unregistered identity or a role the route does not allow is 403; a failed registry lookup is 500. The bunker does not start without A3, and the frontend refuses a secret under 32 characters (`proxy_auth.rs`, `config.rs`, `proxy-signature.ts`). The proxy builds these headers from the session, never from the browser (`[...].ts`). The bunker port is not published (B3). Only `/health` is open |
| T7 | Use the console from another site | B2 | X2 | `SameSite=Strict` on the session cookie, and no CORS layer on the bunker (`server.rs`), so a browser on another origin cannot call it |
| T8 | Have the bunker sign anything for anyone | B3, B4 | X4, X6 | There is no `POST /sign` route. NIP-46 needs a connection token an administrator issued: single-use, 1 hour to 7 days, for a member who can sign, and limited to the kinds it names. There is no "everything" grant. The token secret is shown once and stored as a hash. A connection may sign only the kinds granted, and only while its member may sign (`nip46.rs`, `backend/src/connections_api.rs`; #53) |
| T9 | Read or tamper with NIP-46 traffic | B4 | X5 | Requests and responses are encrypted end to end between app and bunker (NIP-44, or NIP-04 for older clients) and signed (`relay.rs`). Relays saved in the console must be `wss://` except on this machine (`bunker_relays.rs`, `validate`); `NIP46_RELAYS` is used as given, without that check (section 5). The client answers relay NIP-42 AUTH challenges (`relay.rs`). A relay can still drop or delay traffic |
| T10 | Pass an app off as something it is not | B4 | X4 | An app's name, URL and image are marked unverified (`metadata_verified: false`) and never used to decide anything (`nip46.rs`) |
| T11 | Read or change the key, secrets or database on the host | B6 | X8 | Not prevented: see section 5. Containers run as UID 65532 on distroless images with no shell (#16). `BUNKER_NSEC_FILE` keeps the key out of the environment, for deployments that set it instead of `compose.yaml`'s `BUNKER_NSEC` |
| T12 | Ship a vulnerable or tampered image | B6 | X9 | Release images are built from a tag, pass the test suite, and are gated on fixable critical and high Trivy findings before anything is published. Published images are scanned weekly (`docs/releasing.md`, `.github/workflows/release-image.yml`, `trivy-security.yml`). Images are not yet signed (section 5) |
| T13 | Lock everyone out | B2, B3 | X3 | The last administrator cannot be removed. `BANCWR_ADMIN_PUBKEY` seeds an administrator when none exists, and never re-adds or demotes anyone afterwards (`backend/src/main.rs`, `backend/src/db.rs`) |

## 5. Known limits and accepted risks

These are true of Bancwr today. Each names what would change it.

| Limit | Consequence | Changes with |
|-------|-------------|--------------|
| The bunker key (A1) is plain text in the environment or a file, not encrypted at rest | Anyone with host access (X8), or a copy of the file or a backup, has the key | #93 (guidance), #94 (encryption) |
| Every app signs as the one bunker key | "For a member" is attribution in the signing log, not a separate identity. One bunker is one Nostr identity | #103 |
| Sessions and challenges live in frontend memory | A frontend restart signs everyone out. Only one frontend instance is supported | #100 |
| No rate limit on signing | A connected app can sign its granted kinds as fast as it asks | #101 |
| No human approval per event | A connection signs any event of a granted kind without asking anyone | #102 |
| A connection outlives its token | The token's 1 hour to 7 days bounds when it can be used to connect, not how long the connection lasts. A connection ends on `logout`, revocation, or its member's removal or demotion | #98 |
| No backup or restore, and no log export or retention | Losing `./data` loses the registry, connections and log. The log grows without limit | #93 (manual backup guidance), #95, #96 |
| No NIP-44 or NIP-04 encrypt and decrypt over NIP-46 | Apps that need them cannot use Bancwr for direct messages. A functional limit, not a weakness | #97 |
| App metadata is self-reported | An app can name itself anything; Bancwr shows it as unverified | Not planned |
| The member's signer decides which key signs in | Some extensions keep using the key first connected to a site (README, "Signing in"; #70) | Not planned |
| `NIP46_RELAYS` is not checked for `wss://` | An operator who sets it can point the bunker at a remote `ws://` relay. NIP-46 content stays encrypted end to end, but anyone on the path can see who talks to the bunker and when | Not planned: the console's relay setting enforces it |
| TLS is the operator's reverse proxy | Bancwr refuses plain HTTP outside localhost but cannot check the proxy's configuration | Not planned |
| The host is trusted | Whoever controls the host's environment and files controls A1, A3, A4 and A9, and so the bunker | Partly #94 |
| Images are not signed and have no SBOM or checksums | A pulled image cannot yet be verified against the release | #104 |
| No independent security review | This document is the maintainers' own analysis | diogel-io/workspace#175 |

## 6. Findings

Findings from the sign-in decision record's spike (#23):

| Finding | State |
|---------|-------|
| 1. NIP-46 `connect` was open: a client sending no secret got every method | **Closed** in #53: connections are token-gated and limited to granted kinds. The bunker key's sign-in refusal (T2) stays |
| 2. NIP-46 responses were kind 24134, which standard clients do not read | **Closed** in #52: requests and responses are kind 24133 (`relay.rs`) |
| 3. The relay client could not answer NIP-42 AUTH | **Closed** with #53: the client is configured with the bunker's keys and authenticates automatically (`relay.rs`) |

No open findings are recorded here; see [`SECURITY.md`](../SECURITY.md) for how they are handled.

## 7. Verification hooks

| Control | Tests |
|---------|-------|
| T1 sign-in rules, T2 bunker key at sign-in | `frontend/tests/server/auth/login-event.spec.ts`, `sign-in-round-trip.spec.ts` |
| T3 session registry and revocation | `frontend/tests/server/auth/sessions.spec.ts` |
| T3, B1 configuration refusals | `frontend/tests/server/auth/config.spec.ts`, `backend/tests/unit/config_test.rs` |
| T6 proxy signature (the same vectors in both languages) | `frontend/tests/server/auth/proxy-signature.spec.ts`, `backend/tests/unit/proxy_auth_test.rs`, `backend/tests/integration/auth_test.rs` |
| T4, T5 roles and the access matrix | `backend/tests/integration/team_api_test.rs`, `auth_test.rs`, `frontend/e2e/roles.spec.ts` |
| T8, T10 connection tokens and connections | `backend/tests/integration/nip46_connections_test.rs`, `backend/tests/unit/nip46_test.rs`, `frontend/e2e/connections.spec.ts` |
| T9 kind 24133 requests and responses | `backend/tests/integration/nip46_connections_test.rs` |
| T9 NIP-42 AUTH challenge handling | No dedicated test |
| T9 `wss://` required for console-saved relays, except on this machine | `backend/tests/integration/relays_api_test.rs`. `NIP46_RELAYS` is not covered |
| T12 release gates | `.github/scripts/test-release-scripts.sh`, and the release workflows themselves |
| T13 the no-administrator warning | `frontend/tests/server/auth/members.spec.ts`, `backend/tests/unit/health_test.rs` |
| T7 CORS, T11 host | No dedicated test |
