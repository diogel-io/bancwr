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
the vault on every request:

| Route | administrator | user | signer | `service` |
|-------|:---:|:---:|:---:|:---:|
| `GET /health` | open | open | open | open |
| `GET /api/bunker/status` | yes | yes | yes | yes |
| `GET /api/bunker/logs`, `/metrics`, `/config` | yes | | | |
| `GET`, `POST /api/bunker/team`; `DELETE /api/bunker/team/:id` | yes | | | |
| `GET /api/bunker/team/by-pubkey/:pubkey` | yes | | | yes |

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

`status` is `healthy`, `degraded` or `unhealthy`, from three checks in `checks`:

| Check | `pass` | `warn` | `fail` |
|-------|--------|--------|--------|
| `signer` | signs and verifies a throwaway event, never published | | it cannot |
| `database` | the audit log's table answers | | it does not |
| `relays` | every NIP-46 relay is connected | some are, not all | none is, or NIP-46 is on with no relays |

Any `fail` makes the bunker `unhealthy`; otherwise any `warn` makes it `degraded`. No relay
connected is a failure because NIP-46 is the bunker's only signing path. With NIP-46 off, the
relay check is `disabled` and does not count. A relay still connecting counts as not connected, so
for the first seconds after a start the relay check reads red.

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
Adds a new team member. Valid roles are `administrator`, `user` and `signer`. The pubkey may be an
npub or hex; it is stored as hex, and a key already registered, in either form, gets 409. The
bunker's own key cannot be registered.

`DELETE /api/bunker/team/:id` removes a member, except the only administrator (409).

`GET /api/bunker/team/by-pubkey/:pubkey` returns one member by npub or hex, or 404
`{"error":"not_registered"}`.

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
    "pubkey": "npub1...",
    "event_kind": 1,
    "timestamp": "2024-01-01T12:00:00Z"
  }
]
```

## NIP-46 Remote Signing
The bunker supports the NIP-46 remote signing protocol. When enabled, it connects to the specified Nostr relays and listens for signing requests.

### Configuration
Enable NIP-46 and specify relays in your `.env` file or environment variables:

```env
NIP46_ENABLED=true
NIP46_RELAYS=wss://relay.nsecbunker.com,wss://relay.damus.io
```

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

