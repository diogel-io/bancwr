# Running Bancwr in a JetBrains IDE

This guide runs the backend and frontend as local development processes, using the shared run
configurations in `.run/`. It does not use Podman or Docker Compose; its database, secrets, and
encrypted bunker key live under `backend/`, separate from the Compose data and secrets at the
repository root.

## Prerequisites

- Open the repository root in a JetBrains IDE with Rust and JavaScript/TypeScript support.
- Install the stable Rust toolchain specified by `rust-toolchain.toml`.
- Install Node.js and the project-pinned pnpm version (`10.29.3`), then install frontend dependencies:

  ```bash
  cd frontend
  pnpm install
  ```

The shared **Run Frontend** configuration points to
`$USER_HOME$/.local/share/pnpm/bin/pnpm`. If pnpm is installed elsewhere, change the package-manager
path in that configuration (or run `pnpm dev` from `frontend/` in a terminal).

## Configure local environments

Create `backend/.env` for the backend and `frontend/.env` for the frontend. Both files are ignored
by git. Generate a proxy secret once with `openssl rand -hex 32` and put the same value in
`BANCWR_PROXY_SECRET` and `NUXT_PROXY_SECRET`. Generate a different value for
`NUXT_SESSION_PASSWORD`:

`backend/.env`:

```dotenv
BANCWR_PROXY_SECRET=<same generated value as NUXT_PROXY_SECRET>
BANCWR_ADMIN_PUBKEY=<your administrator npub>
BANCWR_ROOT_KEY_FILE=secrets/bancwr_root_key
```

`frontend/.env`:

```dotenv
NUXT_PROXY_SECRET=<same generated value as BANCWR_PROXY_SECRET>
NUXT_SESSION_PASSWORD=<different value from openssl rand -hex 32>
NUXT_SITE_ORIGIN=http://localhost:3001
```

Replace the example values; do not include the angle brackets. Set `BANCWR_ADMIN_PUBKEY` to the
npub of the person who will sign in as the first administrator. It must not be the bunker's npub.
The frontend defaults to the local backend at `http://localhost:3000`, so `NUXT_API_BASE` does not
need to be set for this setup.

## Import the bunker key once

The backend does not run with a plain `nsec`. It encrypts the key into `backend/data/` and uses a
separate root credential file to decrypt it when it starts.

Before importing, check whether `backend/data/bunker-key.enc` already exists. If it does, do not
import again or delete it; use the root credential that belongs to that key. If only the root
credential already exists, keep it and use it rather than generating a replacement.

1. In a terminal, from the repository root, create the backend's private local directories and a
   root credential if one does not already exist. Keep this credential; do not regenerate it after
   importing the key.

   ```bash
   mkdir -p -m 700 backend/secrets backend/data
   if [ ! -e backend/secrets/bancwr_root_key ]; then
     (umask 077; openssl rand -hex 32 > backend/secrets/bancwr_root_key)
   fi
   ```

2. Create a temporary key file with restrictive permissions and enter your bunker `nsec` (or its
   hex secret key). Run this from the repository root:

   ```bash
   (umask 077; ${EDITOR:-nano} backend/secrets/bunker_nsec)
   ```

3. Import it from `backend/`. This uses the root credential path in `backend/.env`, and writes the
   encrypted key beside the local database.

   ```bash
   cd backend
   BUNKER_NSEC_FILE=secrets/bunker_nsec cargo run -- import-key
   ```

   Confirm the printed npub is the expected bunker key. Import never overwrites an existing
   `data/bunker-key.enc`; if one already exists, stop and use the root credential that belongs to
   that key rather than deleting it or importing over it.

4. Remove the temporary plaintext key file and check for editor backups or other copies. Do not
   add `BUNKER_NSEC` or `BUNKER_NSEC_FILE` to either `.env` file or any IDE run configuration.

   ```bash
   rm secrets/bunker_nsec
   ```

The run configuration starts the backend with `backend/` as its working directory, so
`BANCWR_ROOT_KEY_FILE=secrets/bancwr_root_key` resolves to the correct file. Keep the root
credential protected and backed up separately from `backend/data/`; the encrypted key cannot be
unlocked without its original root credential. If the root credential or encrypted key already
exists, preserve it and do not repeat the initialization commands.

## Start and sign in

1. Select **Run Backend** and start it. The first run builds the Rust application; it listens on
   `http://localhost:3000` by default.
2. Select **Run Frontend** and start it. Open <http://localhost:3001>.
3. Check <http://localhost:3000/health>. A healthy response confirms that the bunker unlocked and
   the backend is ready. If it reports a locked state, check the backend log, confirm the root
   credential path and encrypted key are present, and make sure no plain-key environment variable
   is set.
4. Sign in with the Nostr key whose npub is in `BANCWR_ADMIN_PUBKEY` (using a NIP-07 browser
   extension or a NIP-46 bunker connection).

Run the backend before the frontend. Keeping them in separate run configurations makes their logs
and environment easier to inspect; stop each process from the IDE when finished.