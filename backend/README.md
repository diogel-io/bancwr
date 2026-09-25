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

### Health Check
`GET /health`
Returns `{"status": "ok"}` when the server is running. Used for health checks.

### Bunker Status
`GET /api/bunker/status`
Returns the status and public key of the bunker.

### Get Config
`GET /api/bunker/config`
Returns the current configuration (public key and nsec file path if set).

### Update Config
`POST /api/bunker/config`
Updates the bunker's nsec or nsec file path. **Restart is required to apply the new configuration.**

### Team Management
`GET /api/bunker/team`
Returns an array of team members.

`POST /api/bunker/team`
Adds a new team member. Valid roles are `admin`, `signer`, and `viewer`. Pubkey must start with `npub1`.

Example:
```bash
curl -X POST http://localhost:3000/api/bunker/team \
  -H "Content-Type: application/json" \
  -d '{"name":"Alice","pubkey":"npub1...","role":"signer"}'
```

Example:
```bash
curl -X POST http://localhost:3000/api/bunker/config \
  -H "Content-Type: application/json" \
  -d '{"nsec":"nsec1..."}'
```

Example:
```bash
curl http://localhost:3000/api/bunker/status
```

Response:
```json
{
  "status": "healthy",
  "pubkey": "npub1..."
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

### Sign Event
`POST /sign`
Accepts a JSON unsigned event and returns a signed event.

Example:
```bash
curl -X POST http://localhost:3000/sign \
  -H "Content-Type: application/json" \
  -d '{
    "pubkey": "32e18276ca5d706231bfa266c2231571bc950186194165952d79040702d7667d",
    "created_at": 1700000000,
    "kind": 1,
    "tags": [],
    "content": "Hello, Nostr!"
  }'
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
2. Open `tests/endpoints/tests/health/health_get.http` or `tests/endpoints/tests/sign/sign_post.http`.
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

