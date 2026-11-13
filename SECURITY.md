# Security Policy

Bancwr holds a Nostr private key and signs with it for the apps and people its administrators
allow. This file explains how to report a vulnerability and what counts as one. The threat model
behind it, with Bancwr's assets, trust boundaries, controls and known limits, is
[docs/threat-model.md](docs/threat-model.md).

## Reporting a vulnerability

**Do not open a public issue, discussion, or pull request for a suspected vulnerability.**

Report it privately:

1. **Preferred:** use GitHub private vulnerability reporting. Open the repository's
   [Security tab](https://github.com/diogel-io/bancwr/security) and choose **Report a
   vulnerability**.
2. **Alternative:** email support@threenine.co.uk with "Bancwr security" in the subject.

Please include:

- the Bancwr version or image tag (and its digest, if you have it), and how it is deployed;
- the steps to reproduce, and what you expected to happen instead;
- the impact: what an attacker gains and what they need first;
- a proof of concept if you have one. Never use a real private key in a report; generate a
  throwaway key for the bunker and for any member.

What to expect:

| Step | Target |
|------|--------|
| Acknowledgement | Within 3 working days |
| Triage | Within 10 working days: we confirm or decline, and agree a severity |
| Fix and release | As fast as the severity needs; we keep you informed until it ships |
| Disclosure | A GitHub security advisory when the fix is released, by default within 90 days of the report |

We credit reporters in the advisory unless you ask us not to. Please give us the chance to release a
fix before you disclose publicly.

## Supported versions

Bancwr has not been released yet: until `v0.1.0`, fixes land on `master` and reach the `edge`
images. From the first release on, only the newest release receives security fixes, as a new
release; see [docs/releasing.md](docs/releasing.md).

| Version | Supported |
|---------|-----------|
| Newest release (from `v0.1.0`) | Yes |
| `master` and `edge`, before the first release | Yes |
| Any earlier release | No |

## Scope

In scope: anything that defeats a Bancwr control in the threat model, for example

- exposure of the bunker's key, the proxy secret, the session password, or a connection token's
  secret;
- the bunker signing an event without a connection token an administrator issued, or of a kind
  the connection was not granted;
- signing in as a key you do not hold, or keeping a session after signing out;
- calling the bunker's API without the frontend, or as an identity other than your own;
- doing more than your role allows, or keeping access after being removed or demoted;
- secrets reaching logs, error messages, or the network;
- a flaw in the build or release pipeline that could ship altered images.

Out of scope:

- an attacker who already controls the host, its environment, or the bunker's data directory
  (the threat model, section 5, "The host is trusted");
- the known limits listed in the threat model, section 5, unless you show they are worse than
  stated;
- the operator's TLS reverse proxy, or a deployment that ignores the README, such as publishing
  the bunker's port;
- vulnerabilities in relays or Nostr apps themselves, rather than in how Bancwr uses them.

If you are unsure, report it anyway.

## Security review guidelines

A pull request that touches a sensitive path must pass this review before it merges. T and B
numbers refer to [the threat model](docs/threat-model.md).

### Sensitive paths

A change to any of these needs the security review:

- `backend/src/proxy_auth.rs` and `backend/src/server.rs`, which decide who may call each route
- `backend/src/registry.rs` and `backend/src/db.rs`, which hold members, roles, tokens and the audit
  log, and `backend/src/migrations.rs`
- `backend/src/key_store.rs`, `backend/src/signer.rs`, `backend/src/config.rs` and
  `backend/src/main.rs`, which encrypt, decrypt and load the key and the secrets, and seed the first
  administrator
- `backend/src/nip46.rs`, `backend/src/relay.rs`, `backend/src/connections_api.rs`,
  `backend/src/relays_api.rs` and `backend/src/bunker_relays.rs`, the NIP-46 path and its relays
- `frontend/server/api/auth/*` and `frontend/server/utils/auth/*`, sign-in, sessions and the
  proxy identity
- `frontend/server/api/bunker/[...].ts`, `frontend/server/utils/bunker-service.ts` and
  `frontend/server/utils/backend.ts`, the proxy to the bunker
- `compose.yaml`, `.env.example`, `backend/Dockerfile` and `frontend/Dockerfile`
- `backend/Cargo.toml`, `backend/Cargo.lock`, `frontend/package.json`, `frontend/pnpm-lock.yaml`,
  `.trivyignore` and `.github/workflows/*`
- `SECURITY.md`, `docs/threat-model.md` and `docs/releasing.md`, which define the controls, this
  review, and the release gates
- anything else that touches the bunker's key, a secret, signing, or who may call what

### Checklist

**Sign-in and sessions**

- [ ] Sign-in stays a NIP-98 event bound to the URL from `NUXT_SITE_ORIGIN` (never `Host`), to
      `POST`, and to a single-use server challenge, inside the `created_at` window (T1).
- [ ] The bunker's own key can still never sign in, be registered, or be the first administrator
      (T2).
- [ ] The session cookie keeps its flags and holds only a session id; signing out still revokes it
      server-side (T3).

**Who may call what**

- [ ] Every new `/api/bunker/*` route sits in a guarded group in `create_router` and appears in the
      access table in `backend/README.md` and in `backend/tests/integration/auth_test.rs` (T5).
- [ ] Roles and identities come from the verified caller and the registry on every request, never
      from a request body, a query string, or the session cookie (T4, T5).
- [ ] The frontend builds the proxy's identity headers from the session, never from the browser's
      request, and the `service` identity reaches only the routes sign-in needs (T6).
- [ ] The bunker's port stays unpublished, and nothing adds a CORS layer (B3, T7).

**Signing**

- [ ] Nothing signs without a connection token an administrator issued, and a connection signs
      only the kinds it was granted, while its member may sign. There is still no "sign anything"
      route (T8).
- [ ] An app's self-reported name, URL or image decides nothing (T10).

**Secrets**

- [ ] No key, secret, token secret, session id or event content reaches logs, error messages,
      API responses, or the audit log, beyond what the threat model allows.

**Dependencies and build**

- [ ] Every new dependency is justified in the pull request and the lockfile change is reviewed.
- [ ] Workflow and image changes keep the test and Trivy gates in front of publishing, keep
      secrets out of logs, and do not widen token permissions without a reason (T12).

**Tests**

- [ ] Each new or changed control has a negative test: a wrong role, a missing or stale
      signature, an unknown token, a kind not granted, or a secret that must not appear in a
      response.

**Threat model**

- [ ] [docs/threat-model.md](docs/threat-model.md) is updated in the same pull request when the
      change adds or changes an asset, a trust boundary, or a control. An open, exploitable
      weakness appears there only after its fix ships; until then it is reported privately, as
      above.

### Recording the review

1. The pull request description has a **Security review** section. For a change to a sensitive
   path, it works through this checklist, marking any item that does not apply. A change that
   touches no sensitive path says so instead.
2. The reviewer confirms the checklist. Any item the change cannot meet needs a recorded decision
   linked from the pull request, and the decision comes first.
3. Agents apply the same checklist: the Diogel workspace's `review-pr` and `diogel-code-review`
   skills check it for any Bancwr pull request that touches a sensitive path.
4. A release pull request is the exception: its **Security review** section records only that the
   release's security review is complete, at which head commit, and its outcome, as
   [docs/releasing.md](docs/releasing.md#releasing) describes. The checklist evidence for a release
   is in that private review.

Each release also has a security review before its release PR merges; see
[docs/releasing.md](docs/releasing.md#releasing).
