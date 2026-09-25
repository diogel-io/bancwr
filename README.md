
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
| `<version>` | Published when a `v*` tag is released, without the `v` prefix, so `v1.0.0` publishes `1.0.0`. No release has been cut yet. |

Until a release exists, pin to a `sha-` tag if you need a fixed image.

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
2. Edit `.env` and set your `BUNKER_NSEC`.
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
