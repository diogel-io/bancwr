# Bancwr

The Bancwr bunker: a self-hosted Nostr signer that holds an nsec and signs on request,
over HTTP and over NIP-46.

`bunker` is the crate name and the vocabulary used throughout the API and the environment
variables. Bancwr is the product.

## Running
The project supports loading environment variables from a `.env` file in the project root.
The server runs on port 3000 by default, but this can be changed using the `BUNKER_PORT` environment variable.

### From environment variable:
```bash
BUNKER_NSEC=nsec1... BUNKER_PORT=4000 cargo run
```

### From file:
```bash
BUNKER_NSEC_FILE=/path/to/nsec cargo run
```

### From SQLite (logging & configuration):
By default, the bunker uses SQLite to store audit logs and configuration.
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
| `GET /api/bunker/logs/mine` | own | own | | |
| `GET /api/bunker/team` | yes | | yes | |
| `POST /api/bunker/team`; `DELETE /api/bunker/team/:id` | yes | | | |
| `GET /api/bunker/team/by-pubkey/:pubkey` | yes | | yes | yes |
| `GET`, `PUT /api/bunker/relays` | yes | | | |
| `POST`, `GET /api/bunker/connections/tokens`; `DELETE /api/bunker/connections/tokens/:id` | yes | | | |
| `GET /api/bunker/connections` | all | own | | |
| `DELETE /api/bunker/connections/:id` | any | own | | |

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
Returns the public key and, if the key was loaded from a file, that file's path. The configuration
is read-only over the API: the signing key is set with `BUNKER_NSEC_FILE` or `BUNKER_NSEC` when the
bunker starts, and the nsec is never returned.

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

There is no endpoint to change a member's role: remove them and add them again. Wherever a role does
change (today only the `BANCWR_ADMIN_PUBKEY` bootstrap, which promotes), `Database::change_member_role`
applies the rule that a member who becomes a viewer loses their NIP-46 connections (`role_changed`)
and unused tokens (#77).

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
`GET /api/bunker/logs`
Returns the last 100 signing logs in reverse chronological order.

Example:
```bash
curl http://localhost:3000/api/bunker/logs
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
{ "for_pubkey": "npub1…", "label": "Damus on my phone", "kinds": [1, 7], "expires_in_hours": 24 }
```

- `for_pubkey` is the vault member the connection is attributed to (the administrator when
  omitted). It must be a member who can sign, an administrator or a signer: a viewer gets
  `400 {"error":"member_cannot_sign"}` (#77). Removing the member later ends their tokens and
  connections.
- `kinds` are the event kinds the app may sign. Nothing else is ever signed: there is no
  "everything" grant. If the app asks for permissions in `connect`, it gets those of them the
  token allows, and is refused if that leaves nothing.
- The token is valid for 1 hour to 7 days (24 hours by default), and **for one connection only**.
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

Supported methods: `connect`, `get_public_key` (hex), `sign_event`, `ping`, `switch_relays` (no
change), `logout`. The NIP-04 and NIP-44 encryption methods are not supported.

## Podman

### Build & Run with Podman

1. Create a `.env` file with your `BUNKER_NSEC`:
   ```env
   BUNKER_NSEC=nsec1...
   BUNKER_PORT=3000
   ```

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

