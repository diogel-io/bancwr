
# Bancwr

[![CI](https://github.com/diogel-io/bancwr/actions/workflows/ci.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/ci.yml)
[![Release Backend](https://github.com/diogel-io/bancwr/actions/workflows/release-backend.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/release-backend.yml)
[![Release Frontend](https://github.com/diogel-io/bancwr/actions/workflows/release-frontend.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/release-frontend.yml)
[![Trivy Security Scan](https://github.com/diogel-io/bancwr/actions/workflows/trivy-security.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/trivy-security.yml)

## Podman Images

Images are published to GitHub Container Registry (GHCR) on every push to `master`.

Available tags:

| Tag | Points at |
|-----|-----------|
| `latest` | The current `master` commit. This is trunk, not a reviewed release. |
| `master` | The same image as `latest`. |
| `sha-<short>` | One specific commit, for pinning. |
| `<semVer>` | The GitVersion version of that build, such as `0.1.0-49` from `master` or `0.1.0` from the `v0.1.0` tag. |
| `<version>` | Published when a `v*` tag is released, without the `v` prefix, so `v0.1.0` publishes `0.1.0`. No release has been cut yet. |

Until a release exists, pin to a `sha-` tag if you need a fixed image.

Versions, tags, and the release process are described in [docs/releasing.md](docs/releasing.md).

### Backend
```bash
# Pull the current master build
podman pull ghcr.io/diogel-io/bancwr-diogel-backend:latest

# Pin to a specific commit
podman pull ghcr.io/diogel-io/bancwr-diogel-backend:sha-1dbcab3
```

### Frontend
```bash
# Pull the current master build
podman pull ghcr.io/diogel-io/bancwr-diogel-frontend:latest

# Pin to a specific commit
podman pull ghcr.io/diogel-io/bancwr-diogel-frontend:sha-1dbcab3
```

## Running the Environment

1. Create a `.env` file from the example:
   ```bash
   cp .env.example .env
   ```
2. Edit `.env` and set your `BUNKER_NSEC`, and `BANCWR_ADMIN_PUBKEY` to your own npub (see
   [The first administrator](#the-first-administrator)).
3. (Optional) Ensure the Podman socket is running (required for `podman compose`):
   ```bash
   systemctl --user enable --now podman.socket
   ```
4. Start the environment:
   ```bash
   podman compose up -d
   ```

The frontend reaches the bunker over the compose network at `http://bunker:3000`, set as
`NUXT_API_BASE` on the frontend service in `compose.yaml`. If you run the frontend image outside
compose, set `NUXT_API_BASE` to the bunker's address yourself; it defaults to
`http://localhost:3000`, which is the frontend's own port inside the container.

### Reaching the bunker

`compose.yaml` does not publish the bunker's port: only the frontend reaches it, over the compose
network. To check its health from the host:

```bash
podman exec bancwr-bunker curl -fs localhost:3000/health
```

The API behind it is protected by `BANCWR_PROXY_SECRET`, which the frontend also holds, to sign who
is calling. Until sign-in (#11) ships, leave it empty: the frontend cannot sign requests yet, so
setting it locks the dashboard out. Unset, the API is unauthenticated and the bunker logs a warning
at every start. There is no `POST /sign`: it signed any event for anyone who could reach the port.

### The first administrator

Only keys registered with the bunker can use it, each with one of three roles: `administrator`,
`user` or `signer`. Only an administrator can register keys, so the first one comes from the
environment:

- Set `BANCWR_ADMIN_PUBKEY` to your own npub (or hex). It must not be the bunker's key; the bunker
  refuses to start if it is, or if the value is not a valid key.
- At startup, if no administrator is registered, the bunker registers that key as an administrator,
  or promotes it if it is already a member, and logs which.
- Once any administrator exists it does nothing: it never re-adds a key you have removed, and never
  demotes anyone. You can leave it set, but it is tidier to remove it once you have signed in.

Without it, and with no administrator registered, the bunker logs a warning at every start.

Keys are stored in hex and shown as npubs. Upgrading from a version before this change migrates
existing members at startup: the roles `admin` and `viewer` become `administrator` and `user`, and
pubkeys are stored in hex. If the same key was registered twice, once as an npub and once as hex,
the older registration is kept and the other is removed; the log names both.
