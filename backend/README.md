# Bancwr

The Bancwr bunker: a self-hosted Nostr signer that holds an nsec and signs on request,
over HTTP and over NIP-46.

`bunker` is the crate name and the vocabulary used throughout the API and the environment
variables. Bancwr is the product.

## Running
The project supports loading environment variables from a `.env` file in the project root.
The server runs on port 3000 by default, but this can be changed using the `BUNKER_PORT` environment variable.

### The key, encrypted at rest (#94)

The bunker never runs from a plain key. Its key is in a file encrypted with XChaCha20-Poly1305
under a root credential, 64 hex characters in the file `BANCWR_ROOT_KEY_FILE` names. Create them
once:

```bash
mkdir -p -m 700 secrets data
(umask 077; openssl rand -hex 32 > secrets/bancwr_root_key)
(umask 077; ${EDITOR:-nano} secrets/bunker_nsec)   # the key, as an nsec or in hex
BUNKER_NSEC_FILE=secrets/bunker_nsec BANCWR_ROOT_KEY_FILE=secrets/bancwr_root_key cargo run -- import-key
rm secrets/bunker_nsec
```

`import-key` writes `bunker-key.enc` beside the database (`BANCWR_KEY_PATH` sets another path),
prints the npub, and never replaces an existing key file. It reads `BUNKER_NSEC_FILE` first, then
`BUNKER_NSEC`. Then run the bunker with neither set:

```bash
BANCWR_ROOT_KEY_FILE=secrets/bancwr_root_key BUNKER_PORT=4000 cargo run
```

While `BUNKER_NSEC` or `BUNKER_NSEC_FILE` is set, or the root credential is missing or wrong, or the
key file is missing or altered, the bunker runs locked: it signs nothing, NIP-46 does not start,
`/health` answers `503` with `{"status":"locked","reason":"..."}`, `bunker healthcheck` prints the
reason, and `GET /api/bunker/status` reports the `signer` check failed with it. Only the status and
the member lookup that sign-in needs answer; every other route answers `503`. Under compose, see
[docs/operating.md](../docs/operating.md#protecting-the-bunker-key), which also covers migrating a
0.1.0 bunker and backing up and restoring the bunker.

`bunker backup` writes an encrypted archive of a consistent database snapshot and the key file to
standard output, safe while the bunker runs. `bunker restore` reads one from standard input into an
empty data directory and revokes every NIP-46 connection and unused token (#95). Both read the
backup passphrase, 16 characters or more and not the root credential, from the file
`BANCWR_BACKUP_KEY_FILE` names; `restore` also needs the root credential, and checks that it
decrypts the archived key before writing anything:

```bash
(umask 077; openssl rand -base64 24 > secrets/bancwr_backup_key)
BANCWR_BACKUP_KEY_FILE=secrets/bancwr_backup_key cargo run -- backup > bancwr.bcwr
BANCWR_BACKUP_KEY_FILE=secrets/bancwr_backup_key BANCWR_ROOT_KEY_FILE=secrets/bancwr_root_key \
  DATABASE_PATH=restored/bancwr.db cargo run -- restore < bancwr.bcwr
```

### From SQLite (logging & configuration):
By default, the bunker uses SQLite to store the signing log, audit events (#96) and configuration.
SQLite is compiled into the binary via `rusqlite` with the `bundled` feature, so no
system SQLite installation is required.

The database file is created at `./data/bancwr.db` by default.
You can change the path using the `DATABASE_PATH` environment variable.

```bash
DATABASE_PATH=./custom_path/bunker.db cargo run
```

The runtime database and its WAL sidecar files (`-wal`, `-shm`) are excluded from
version control. Do not commit database files; use explicit seed fixtures instead.

## API

### Authentication and roles

This boundary, and the others Bancwr relies on, are described in the
[threat model](../docs/threat-model.md) (B3, T6).

The browser never calls the bunker: it calls the frontend's Nitro server, which proxies
`/api/bunker/*` and attaches the caller's identity. When `BANCWR_PROXY_SECRET` is set, every
`/api/bunker/*` request must carry:

| Header | Value |
|--------|-------|
| `x-bancwr-identity` | the signed-in user's pubkey, lowercase hex, or `service` (the frontend acting for itself during sign-in) |
| `x-bancwr-timestamp` | Unix time, within 30 s of the bunker's clock |
| `x-bancwr-signature` | lowercase hex HMAC-SHA256 of `v1\n<timestamp>\n<METHOD>\n<path?query>\n<identity>` under `BANCWR_PROXY_SECRET` |

The frontend holds the same secret as `NUXT_PROXY_SECRET`. The bunker then looks the pubkey up in
the vault on every request.

#### Access by role (#77)

The roles are `administrator`, `signer` and `viewer` (shown as Admin, Signer and Viewer).
Administrator is a superset; a viewer reads the team and never holds a NIP-46 connection. The
matrix lives in one place, `Access` in `src/proxy_auth.rs`, and `src/server.rs` puts each route in
its group:

| Route | administrator | signer | viewer | `service` |
|-------|:---:|:---:|:---:|:---:|
| `GET /health` | open | open | open | open |
| `GET /api/bunker/status` | yes | yes | yes | yes |
| `GET /api/bunker/logs`, `/metrics`, `/config` | yes | | | |
| `GET /api/bunker/logs/export`; `GET`, `PUT /api/bunker/logs/retention` | yes | | | |
| `GET /api/bunker/logs/mine` | own | own | | |
| `GET /api/bunker/team` | yes | | yes | |
| `POST /api/bunker/team`; `DELETE`, `PATCH /api/bunker/team/:id` | yes | | | |
| `GET /api/bunker/team/by-pubkey/:pubkey` | yes | | yes | yes |
| `GET`, `PUT /api/bunker/relays` | yes | | | |
| `POST`, `GET /api/bunker/connections/tokens`; `DELETE /api/bunker/connections/tokens/:id` | yes | | | |
| `GET /api/bunker/connections` | all | own | | |
| `DELETE /api/bunker/connections/:id` | any | own | | |
| `GET /api/bunker/approvals` | all | own | | |
| `POST /api/bunker/approvals/:id/approve`, `/reject` | yes | | | |

Refusals: `401 {"error":"not_authenticated","reason":…}` without a valid signature;
`403 {"error":"not_registered","npub":…}` for a key not in the vault; `403 {"error":"forbidden"}`
for a role the route does not allow.

`BANCWR_PROXY_SECRET` is required: the bunker does not start without it (#11). The bunker has no
CORS headers: browsers never call it directly.

The examples below leave the three headers out for brevity; every `/api/bunker/*` request needs
them.

### Health Check
`GET /health`
Returns `{"status": "ok"}` when the server is running. Used for health checks. Never authenticated.

### Bunker Status
`GET /api/bunker/status`
Returns the bunker's health, public key and version. It answers 200 whatever the health: it is a
report, not a probe. Container probes use `/health`, which touches nothing, so a flapping relay
never restarts the container.

`status` is `healthy`, `degraded` or `unhealthy`, from four checks in `checks`:

| Check | `pass` | `warn` | `fail` |
|-------|--------|--------|--------|
| `signer` | signs and verifies a throwaway event, never published | | it cannot |
| `database` | the audit log's table answers | | it does not |
| `relays` | every NIP-46 relay is connected | some are, not all | none is, or NIP-46 is on with no relays |
| `administrator` | an administrator is registered | none is, so nobody can manage the bunker (set `BANCWR_ADMIN_PUBKEY`) | the team could not be read |

Any `fail` makes the bunker `unhealthy`; otherwise any `warn` makes it `degraded`. No relay
connected is a failure because NIP-46 is the bunker's only signing path. With NIP-46 off, the
relay check is `disabled` and does not count. A relay still connecting counts as not connected, so
for the first seconds after a start the relay check reads red.

The frontend reads a `warn` on `administrator` as "no administrator yet": a key that signs in but
is not registered is then told how the first administrator is set, not to ask one (#74).

Every role can read status, so `detail` is fixed wording plus relay URLs: never raw errors, paths
or keys. The raw error is in the bunker log.

### Get Config
`GET /api/bunker/config`
Returns the public key and `key_storage`, which is always `encrypted` (#94). The configuration is
read-only over the API: the signing key is imported with `bunker import-key`, and neither the key
nor where it is stored is ever returned.

### Team Management
`GET /api/bunker/team`
Returns an array of team members.

`POST /api/bunker/team`
Adds a new team member. Valid roles are `administrator`, `signer` and `viewer` (#77); older names,
such as `user`, are refused with 400. The pubkey may be an
npub or hex; it is stored as hex, and a key already registered, in either form, gets 409. The
bunker's own key cannot be registered.

`DELETE /api/bunker/team/:id` removes a member, except the only administrator (409).

`GET /api/bunker/team/by-pubkey/:pubkey` returns one member by npub or hex, or 404
`{"error":"not_registered"}`.

`PATCH /api/bunker/team/:id` with `{"role":"administrator"|"signer"|"viewer"}` changes a member's
role in place (#99). A role that is not one of the three is 400, an id that is not a UUID is 400, an
unknown member is 404, and demoting the only administrator is 409. Setting the role a member already
has is 200 and changes nothing. A member who becomes a viewer loses their NIP-46 connections
(`role_changed`) and unused tokens (#77); a member who keeps a role that signs keeps them. The change
and any revocations are recorded with the administrator who made them, in one transaction
(`Database::change_member_role_as`, which the `BANCWR_ADMIN_PUBKEY` bootstrap also uses to promote).

### Your own signatures (#77)
`GET /api/bunker/logs/mine` (administrators and signers) returns the caller's own 20 most recent
signatures, newest first, in the same shape as `GET /api/bunker/logs`: those made through a
connection attributed to the caller (`member_pubkey`), never another member's. An administrator sees
only their own here; everyone's is on `GET /api/bunker/logs`. The signer dashboard shows them.

Example:
```bash
curl -X POST http://localhost:3000/api/bunker/team \
  -H "Content-Type: application/json" \
  -d '{"name":"Alice","pubkey":"npub1...","role":"signer"}'
```

Example:
```bash
curl http://localhost:3000/api/bunker/status
```

Response:
```json
{
  "status": "degraded",
  "pubkey": "npub1...",
  "version": "0.1.0",
  "checks": [
    { "name": "signer", "status": "pass", "detail": "The signing key signs and verifies." },
    { "name": "database", "status": "pass", "detail": "The database answers." },
    {
      "name": "relays",
      "status": "warn",
      "detail": "1 of 2 relays connected. Not connected: wss://relay.damus.io",
      "relays": [
        { "url": "wss://relay.nsecbunker.com", "connected": true },
        { "url": "wss://relay.damus.io", "connected": false }
      ]
    }
  ]
}
```

### Get Logs
`GET /api/bunker/logs?limit=&before=`
Returns one page of the signing log, newest first (#96). Without parameters, the newest 100, as
before. `limit` is 1 to 500. `before` continues after a page: the last entry's `timestamp` and
`id`, joined by `|` (`2026-06-01T12:00:00.000000+00:00|<id>`), URL-encoded. A short page is the
last. A bad `limit` is `400 invalid_limit`; a `before` that is not a timestamp and an id is
`400 invalid_cursor`.

Example:
```bash
curl http://localhost:3000/api/bunker/logs
curl 'http://localhost:3000/api/bunker/logs?limit=500&before=2026-06-01T12%3A00%3A00.000000%2B00%3A00%7C<id>'
```

Response:
```json
[
  {
    "id": "...",
    "event_id": "...",
    "pubkey": "<hex>",
    "event_kind": 1,
    "timestamp": "2024-01-01T12:00:00Z",
    "member_pubkey": "<hex>",
    "member_name": "Alice",
    "connection_id": "..."
  }
]
```

- `pubkey` is the key that asked for the signature: the NIP-46 app's key, not the member's.
- `member_pubkey` is the vault member the app's connection was made for, and `connection_id` the
  connection that signed (diogel-io/workspace#38). Entries logged before that change are
  attributed at upgrade to the connection the app held when it signed; any no connection accounts
  for have both as `null`.
- `member_name` is that member's name, read when the log is, so it is `null` once they have been
  removed.

### Export the log and audit events (#96)
`GET /api/bunker/logs/export?format=csv|jsonl&from=YYYY-MM-DD&to=YYYY-MM-DD`
Downloads every signature and every audit event between two UTC dates, both included, oldest
first, as `text/csv` (RFC 4180, CRLF, with a header row) or `application/x-ndjson` (one JSON
object per line). `format` defaults to `csv`. A missing, malformed or reversed range is
`400 invalid_range`; another format is `400 invalid_format`. The body is streamed in batches of
1,000 rows, with `Content-Disposition: attachment; filename="bancwr-log-<from>-<to>.<ext>"`. The
export is itself recorded as an audit event before its rows are read, so it appears in its own
output.

Each row has these fields, in this order:

| Field | Signature | Audit event |
|-------|-----------|-------------|
| `time` | When it was signed, UTC, `YYYY-MM-DDTHH:MM:SS.ffffffZ` | When it was recorded, the same |
| `source` | `signature` | `audit` |
| `action` | `signed` | One of the actions below |
| `event_kind` | The kind signed | empty |
| `event_id` | The signed event's id | empty |
| `member_pubkey`, `member_name` | The member the connection is for, with their name now (empty once removed) | The member it concerns, with their name when it was recorded |
| `connection_id`, `token_id` | The connection that signed | The connection or token it concerns |
| `client_pubkey` | The NIP-46 app's key that asked | The app's key, for a connection |
| `actor_pubkey` | empty | Who did it; empty for the bunker itself or the app |
| `detail` | empty | Fixed values: a reason, permissions, a role change, counts, a peer's key |

Event content is never recorded or exported, and neither is a decrypt's plaintext or ciphertext. In
CSV, a value that starts with `=`, `+`, `-`, `@`, a
tab or a carriage return gets a leading `'`, so a member's or app's name cannot run as a formula in
a spreadsheet.

Audit actions, each recorded in the same transaction as the change:

| `action` | When | `detail` |
|----------|------|----------|
| `token_issued` | An administrator issues a connection token | `perms=…;expires_at=…`, then `;connection_lifetime_hours=…` when it has one, and `;rate_per_minute=…;rate_burst=…` when its rate limit is not the default (#101) |
| `token_revoked` | A token is revoked, or ends with its member's removal, demotion or a restore | `reason=revoked`, `member_removed`, `role_changed` or `restored` |
| `connection_created` | An app connects with a token | `perms=…`, then `;expires_at=…` when it has a lifetime |
| `connection_revoked` | A connection ends | `reason=revoked`, `logout`, `replaced`, `member_removed`, `role_changed`, `restored` or `expired` |
| `member_added`, `member_removed` | Team changes | `role=…` |
| `member_role_changed` | An administrator changes a role (#99), or the `BANCWR_ADMIN_PUBKEY` bootstrap promotes | `from=…;to=…` |
| `relays_changed` | An administrator saves the bunker relays | `relays=…` |
| `retention_changed` | An administrator sets the retention policy | `max_age_days=…;max_rows=…` |
| `logs_pruned` | The policy deleted rows | `signatures=…;audit_events=…` |
| `logs_exported` | An administrator exports | `format=…;from=…;to=…` |
| `restored` | `bunker restore` revoked connections and tokens | `connections=…;tokens=…` |
| `nip44_decrypt`, `nip04_decrypt` | A NIP-46 app asks the bunker to decrypt (#97) | `peer=…`: the hex key of the other party |
| `approval_requested` | A `sign_event` of a kind that needs approval is held (#102) | `request=…;kind=…;event_id=…` |
| `approval_approved`, `approval_rejected` | An administrator decides a held request (#102) | `request=…;kind=…;event_id=…`, then `;note=…` when given |
| `approval_expired` | Nobody decided in time (#102) | `request=…;kind=…;event_id=…;timeout_seconds=…` |
| `approval_cancelled` | The request's connection ended while it waited (#102) | `request=…;kind=…;event_id=…;reason=connection_ended` |
| `connection_rate_limited` | A NIP-46 app is refused for going over its connection's rate limit (#101), at most once a minute per connection | `rate_per_minute=…;burst=…` |

A decrypt changes nothing, so it has no transaction of its own: its event is written before the
bunker decrypts, and if that write fails nothing is decrypted. The event names the member, the
connection, its token and the app, and has no actor. A decrypt that fails, such as a malformed
ciphertext, is still recorded. Encrypting is not recorded: it shows the app nothing, and a message
the app then sends is signed, and that signature is.

Upgrading to this version adds the audit events table and fills it from what the database already
held: each token's issue and revocation and each connection's creation and revocation, with the
administrator where a column recorded one. Changes to the team before the upgrade were never
recorded, so they are not there.

Example:
```bash
curl -OJ 'http://localhost:3000/api/bunker/logs/export?format=csv&from=2026-06-01&to=2026-06-30'
```

### Log retention (#96)
`GET /api/bunker/logs/retention` returns the policy, and `PUT` sets it:

```json
{ "max_age_days": 365, "max_rows": null, "min_age_days": 30, "min_rows": 10000 }
```

`max_age_days` deletes rows older than that many days; `max_rows` deletes the oldest rows over that
many. Each applies to the signing log and to the audit events separately. `null` is no limit, and
both `null`, the default, keeps everything. A policy under 30 days or 10,000 rows is refused
(`400 invalid_max_age`, `400 invalid_max_rows`), so a mistake cannot wipe the trail. `PUT` takes
`max_age_days` and `max_rows`, applies the policy at once, and returns it. The bunker applies it
again at start and every hour, deleting 1,000 rows at a time, and records each change and each
prune that deleted anything.

## NIP-46 Remote Signing
The bunker supports the NIP-46 remote signing protocol (read at nips commit `0046368a`). When
enabled, it connects to the specified Nostr relays and answers requests addressed to its key, as
kind 24133 events encrypted with NIP-44 (or NIP-04, which older clients still send; each is
answered in the scheme it used). It answers relays' NIP-42 AUTH challenges with its key.

### Configuration
Enable NIP-46 in your `.env` file or environment variables:

```env
NIP46_ENABLED=true
```

Then choose the bunker's relays in the console (see [Bunker relays](#bunker-relays-78)), or pin
them with `NIP46_RELAYS`, which overrides the console:

```env
NIP46_RELAYS=wss://relay.nsec.app,wss://relay.damus.io
```

With NIP-46 on and no relays from either, the bunker still starts and listens for a list to be
saved; until then the relay check is red and no token can be issued.

### Bunker relays (#78)
The relays the bunker reaches NIP-46 apps through, and that every `bunker://` string carries. They
are the bunker's own, kept in its database (`bunker_relays`) and never published to Nostr; members'
NIP-65 relay lists are separate.

Which list is in force:

- **`NIP46_RELAYS`, when set and non-empty, always decides** (`source: "environment"`). A list saved
  in the console is then ignored (the bunker logs this at startup), and cannot be changed over the
  API. Unset the variable and restart to manage the relays in the console. There is no seeding:
  the variable's relays are never copied into the database.
- Otherwise, **the list an administrator saved** (`source: "console"`).

Token `bunker://` strings, the `relays` health check and the running relay client all use the list
in force.

`GET /api/bunker/relays` (administrators) returns it in order, with whether each relay is connected
now:

```json
{
  "source": "console",
  "nip46_enabled": true,
  "relays": [
    { "url": "wss://relay.nsec.app", "connected": true },
    { "url": "wss://relay.damus.io", "connected": false }
  ]
}
```

`PUT /api/bunker/relays` (administrators) with `{ "relays": ["wss://…", …] }` replaces the saved
list whole, in that order, applies it to the running relay client at once (new relays are added,
connected and subscribed to NIP-46 requests; dropped ones are disconnected), and answers as `GET`
does. Addresses are normalised (host lowercased, a trailing slash on an empty path dropped). It is
refused, with `{ "error", "message" }`, and nothing saved:

| Status | `error` | When |
|--------|---------|------|
| 409 | `relays_from_environment` | `NIP46_RELAYS` is set |
| 400 | `invalid_relay_url` | not `wss://`, or `ws://` other than `localhost`, `127.0.0.1` or `[::1]`; credentials or a `#fragment`; over 255 characters |
| 400 | `duplicate_relay` | the same relay twice, after normalising |
| 400 | `too_many_relays` | more than 6 |
| 400 | `no_relays` | an empty list while NIP-46 is on |

An app learned the bunker's relays from its `bunker://` string. Removing a relay it uses cuts it off
until it reconnects with a new string, which always carries the current list; the console warns
before such a save.

### Connecting an app (#53)
Turning NIP-46 on connects nothing by itself: an app connects only with a **connection token** an
administrator issues.

```bash
POST /api/bunker/connections/tokens
{ "for_pubkey": "npub1…", "label": "Damus on my phone", "kinds": [1, 7], "methods": ["nip44_encrypt", "nip44_decrypt"], "expires_in_hours": 24 }
```

- `for_pubkey` is the vault member the connection is attributed to (the administrator when
  omitted). It must be a member who can sign, an administrator or a signer: a viewer gets
  `400 {"error":"member_cannot_sign"}` (#77). Removing the member later ends their tokens and
  connections.
- `kinds` are the event kinds the app may sign. Nothing else is ever signed: there is no
  "everything" grant.
- `methods` are the encryption methods it may call (#97), any of `nip44_encrypt`, `nip44_decrypt`,
  `nip04_encrypt` and `nip04_decrypt`; none when omitted. Anything else is
  `400 {"error":"invalid_methods"}`. A token needs at least one kind or method, or it is
  `400 {"error":"invalid_kinds"}`, so a token may allow encryption alone.
- The token's permissions are stored as NIP-46 writes them: `sign_event:<kind>` for each kind, and
  each method by its name. If the app asks for permissions in `connect`, it gets those of them the
  token allows, and is refused if that leaves nothing.
- The token is valid for 1 hour to 7 days (24 hours by default), and **for one connection only**.
- `connection_lifetime_hours` (#98), 1 to 8,760 (365 days), is how long the connection lasts,
  counted from when the app connects; anything else is `400 {"error":"invalid_lifetime"}`. Omitted,
  the connection lasts until it is revoked, as before. The token's own validity only bounds when it
  can be used to connect. A lifetime cannot be extended: issue a new token.
- `rate_per_minute`, 1 to 600, and `rate_burst`, 1 to 100, are the connection's rate limit (#101).
  Each defaults on its own, to 60 and 20; anything else is `400 {"error":"invalid_rate_limit"}`. See
  [Rate limits](#rate-limits-101).
- `approval_kinds` (#102) are the kinds held for an administrator's approval before signing. Each
  must be one of `kinds`, or it is `400 {"error":"invalid_approval_kinds"}`; none when omitted. See
  [Approval before signing](#approval-before-signing-102).
- The response's `uri` is the `bunker://` string to give the app. It carries the token's secret,
  which is shown this once: the bunker stores only its SHA-256 hash.

Every app signs as **the bunker's key**: "for a member" is attribution, not a separate key.
Connections are stored, so they survive a restart. `GET /api/bunker/connections` lists them (an
administrator sees all; a signer only those for their key), with the app's self-reported name,
URL and image marked unverified (`metadata_verified: false`): NIP-46 lets an app name itself, and
that is never used to decide anything. A connection ends when the app sends `logout`, an
administrator (any) or the member it was made for (their own) revokes it
(`DELETE /api/bunker/connections/:id`; another member's is a 404, and `revoked_by` records who),
or its member is removed (`member_removed`) or becomes a viewer (`role_changed`, #77). A request
from a connection whose member is now a viewer is refused as a removed member's is, and a token
issued for one does not connect.

A connection with a lifetime (#98) has an `expires_at`, and `null` without one. From that moment
every request is refused. The first is answered `This connection has expired`, and the connection
is marked revoked with the reason `expired`, no `revoked_by`, and `revoked_at` equal to its
`expires_at`, so the record shows when access ended. That is recorded as `connection_revoked`,
`reason=expired`, and logged. Later requests get `Not connected`, as any ended connection's do. The
bunker also marks expired connections at start, every hour, before listing connections, and before
an app connects again, so one that is never used again is still shown as ended.
Tokens and the token just issued return `connection_lifetime_hours`, `null` when there is none.
Tokens, connections and the token just issued return `rate_per_minute` and `rate_burst`, the limit
in force (#101), and `approval_kinds` (#102).

Supported methods: `connect`, `get_public_key` (hex), `sign_event`, `ping`, `switch_relays` (no
change), `logout`, and the encryption methods below. Tokens, connections and the token just issued
list `kinds` and `methods` separately.

### Encryption (#97)
`nip44_encrypt`, `nip44_decrypt`, `nip04_encrypt` and `nip04_decrypt` take
`[<third_party_pubkey>, <payload>]`, the pubkey in hex, and encrypt or decrypt with the bunker's
key: NIP-44 version 2, or NIP-04 for older apps. Each is a separate permission. A connection without
it gets `Forbidden: this connection may not call <method>`, and nothing is encrypted, decrypted or
recorded.

The other party may be anyone, or the bunker's own key (NIP-51 private lists encrypt to oneself),
but never the key of a NIP-46 app that has connected to this bunker, active or not:
`Forbidden: <method> is not allowed with a NIP-46 app key`. The traffic between apps and the bunker
is encrypted with the bunker's key, so without this an app could read another app's requests and
answers, or write some of its own.

A decrypt grant covers every message sent to the bunker's key, from anyone: it cannot be limited to
particular senders. Each decrypt is recorded (see the audit actions above). A NIP-17 inbox decrypts
twice for each message, a gift wrap and then its seal, so an app reading direct messages adds
audit events quickly; a retention policy bounds them.

### Rate limits (#101)
Each connection may use the bunker's key only so fast, so a runaway bot or a compromised app cannot
sign or decrypt without limit. NIP-46 traffic arrives over relays, so no reverse proxy can do this:
the bunker does it, per connection, as a token bucket. The bucket holds `rate_burst` requests and
refills at `rate_per_minute`; a request that finds it empty is refused.

- **What counts:** `sign_event`, `nip44_encrypt`, `nip44_decrypt`, `nip04_encrypt` and
  `nip04_decrypt`, the methods that use the key for the app. `connect`, `ping`,
  `get_public_key`, `switch_relays` and `logout` do not.
- **When:** once the request's connection is found, before its permissions are checked and before
  any work. A flood of requests for kinds the app may not sign is limited too, and a refused decrypt
  decrypts and records nothing.
- **The limit:** set when the token is issued, and copied to the connection. 60 a minute with a
  burst of 20 by default; from 1 to 600 a minute, with a burst of 1 to 100. There is no unlimited
  setting. A connection's limit cannot be changed: issue a new token.
- **A refusal** is answered
  `Rate limited: this connection may make <n> requests a minute (burst <b>). Try again in <s> s`.
  The first refusal, and then at most one a minute per connection however hard the app pushes, is
  logged as a warning (connection, member and app key) and recorded as `connection_rate_limited`
  with no actor. If recording fails, that is logged, and the request is still refused.
- **The count:** `GET /api/bunker/metrics` returns `nip46_rate_limited`, the requests refused since
  the bunker started, beside `http_requests`, `nip46_connections` and `total_signatures`. The
  dashboard shows it.
- **In memory:** the buckets are kept in memory. A restart refills them all, and the hourly sweep
  drops those that are full. Each connection has its own bucket, so a member with several
  connections has several.

Not limited: a refused request still costs the bunker a decrypt of the request, an encrypted reply
and a relay event, so the limit bounds what the app gets done, not relay traffic. Requests from keys
with no active connection are not limited either: each is refused as `Not connected` after one
indexed lookup.

Upgrading: tokens and connections from before #101 have no stored limit, so they get the default,
60 a minute with a burst of 20, from the upgrade (migration 11).

### Approval before signing (#102)
A token can mark some of its kinds as needing approval (`approval_kinds`). A `sign_event` for one of
them is not signed: it is held until an administrator approves or rejects it in the console, under
**Approvals**. Other kinds sign as before, and so do all connections made without approval kinds.

- **The app waits.** The bunker answers a held request at once with a NIP-46 auth challenge,
  `{"id":…,"result":"auth_url","error":"<console>/approvals/<id>"}`, then sends the real answer later
  under the same request id. `<console>` is `BANCWR_CONSOLE_URL`, which compose sets from
  `NUXT_SITE_ORIGIN`; without it no challenge is sent, and the request still waits. nostr-tools'
  `BunkerSigner` waits for the answer with no time limit of its own; rust-nostr's `nostr-connect`
  waits as long as the app's own request timeout. An app that gives up first never sees the answer.
- **Checks, in order:** the rate limit (#101), so held requests count against it; the permission
  for the kind; then the hold. At most 20 requests wait for one connection and 200 for the bunker;
  past that, `Too many requests are waiting for approval`. Each hold is recorded as
  `approval_requested` first, and refused if that cannot be written.
- **`GET /api/bunker/approvals`** lists the requests waiting, oldest first: all for an
  administrator, their own for a signer. Each shows the event as it will be signed (`kind`,
  `content`, `tags`, `created_at` and `event_id`, under the bunker's key), the member, the
  connection, the app's self-reported name, and when it was asked and times out.
- **`POST /api/bunker/approvals/:id/approve`** with `{"event_id":"…","note":"…"}`, for
  administrators. `event_id` must be the one shown, or it is `409 {"error":"event_changed"}`, and
  the request still waits. The connection is checked again: if it has ended, or may no longer sign
  the kind, the request is cancelled, the app told, and it is `409 {"error":"connection_ended"}`.
  Otherwise the event is signed, logged as a signature, recorded as `approval_approved` with the
  administrator, and sent to the app.
- **`POST /api/bunker/approvals/:id/reject`** with `{"note":"…"}` (optional), for administrators.
  Nothing is signed; the app is told `Rejected by an administrator`, then `: <note>`.
- Either on a request no longer waiting (decided, timed out or unknown) is `404`. Notes are one
  line of at most 280 characters, recorded in the audit `detail`, and shown to the app on a
  rejection.
- **Timeouts.** A request nobody decides within `BANCWR_APPROVAL_TIMEOUT_MINUTES` (1 to 1,440,
  30 by default) ends: the app is told `Approval timed out after <n> minutes: nothing was signed`,
  recorded as `approval_expired`. One whose connection ends while it waits (logout, revoked,
  expired, its member removed or made a viewer, a restore) is cancelled within 15 seconds:
  `This connection ended: nothing was signed`, recorded as `approval_cancelled`.
- **In memory only.** Waiting requests, with their content, are kept in the bunker's memory and
  never written to disk or into a backup. A restart drops them: the app gets no answer, and the
  audit log keeps the `approval_requested` with no outcome.
- **Signed after the app gave up.** If an app's own timeout is shorter and it gives up, a later
  approval still signs and sends the event, but the app is not listening, so it is never
  published. The signature is still in the signing log.
- **Who decides.** Any administrator, including for their own apps' requests. Signers see their own
  requests waiting, without the buttons. `GET /api/bunker/metrics` returns `approvals_pending`.

## Podman

### Build & Run with Podman

1. Create the root credential and import the key, as
   [docs/operating.md](../docs/operating.md#a-new-installation) describes, and a `.env` file from
   `../.env.example`.

2. Ensure the Podman socket is running (required for `podman compose`):
   ```bash
   systemctl --user enable --now podman.socket
   ```

3. Start the service using Podman Compose:
   ```bash
   podman compose up -d
   ```

4. (Optional) Build the image with a different default port:
   ```bash
   podman build --build-arg DEFAULT_PORT=4000 -t bunker .
   ```

5. Check health:
   ```bash
   podman inspect --format='{{json .State.Health}}' $(podman compose ps -q bunker)
   ```

### Using GHCR

Prebuilt images are published to GitHub Container Registry. The pull commands and the
meaning of each tag are in the [root README](../README.md#podman-images); they are not
repeated here so there is only one place for them to go stale.

## Testing

To run all tests (unit and integration):

```bash
cargo test
```

To run only integration tests:

```bash
cargo test --test api_test
```

To run only unit tests:

```bash
cargo test --test config_test
cargo test --test signer_test
cargo test --test server_test
```

### Endpoint Tests

If you use an IDE that supports `.http` files (like RustRover or IntelliJ), you can run the endpoint tests located in `tests/endpoints/tests/`.

1. Ensure the server is running (e.g., `cargo run`).
2. Open `tests/endpoints/tests/health/health_get.http`.
3. Select the `local` environment from the environment selector.
4. Run the requests.

## Development

- Business logic should be in `src/` modules.
- `main.rs` should remain minimal.
- All tests live in the `tests/` directory.
- Use `RUSTFLAGS="-D warnings" cargo clippy` for linting.
- The crate is named `bunker`; the product is Bancwr.
- The database layer lives in `src/db.rs` and uses `rusqlite` with bundled SQLite.
  All UUIDs are stored as TEXT via `Uuid::to_string()` and all timestamps as TEXT
  via `DateTime<Utc>::to_rfc3339()`.

