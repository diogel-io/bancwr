
# Bancwr-Diogel

[![CI](https://github.com/diogel-io/bancwr/actions/workflows/ci.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/ci.yml)
[![Release Backend](https://github.com/diogel-io/bancwr/actions/workflows/release-backend.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/release-backend.yml)
[![Release Frontend](https://github.com/diogel-io/bancwr/actions/workflows/release-frontend.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/release-frontend.yml)
[![Trivy Security Scan](https://github.com/diogel-io/bancwr/actions/workflows/trivy-security.yml/badge.svg)](https://github.com/diogel-io/bancwr/actions/workflows/trivy-security.yml)

## Podman Images

Images are published to GitHub Container Registry (GHCR).

### Backend
```bash
# Pull latest
podman pull ghcr.io/threenine/bancwr-diogel-backend:latest

# Pull specific version
podman pull ghcr.io/threenine/bancwr-diogel-backend:v1.0.0
```

### Frontend
```bash
# Pull latest
podman pull ghcr.io/threenine/bancwr-diogel-frontend:latest

# Pull specific version
podman pull ghcr.io/threenine/bancwr-diogel-frontend:v1.0.0
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
