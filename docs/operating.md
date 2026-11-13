# Operating Bancwr: the key and the database

This page is for whoever runs a Bancwr bunker. It covers the bunker's key, which is encrypted at
rest from 0.2.0 (#94), moving a 0.1.0 bunker to it, and backing up and restoring the database.
Bancwr has no built-in backup yet (#95).

Commands are given for rootless Podman and for Docker. They assume the `compose.yaml` layout from
the [README](../README.md#running-the-environment): the bunker's data in `./data` and its secrets
in `./secrets`, beside `compose.yaml`, and both containers running as UID 65532.

## Protecting the bunker key

The bunker keeps its key in `./data/bunker-key.enc`, encrypted with XChaCha20-Poly1305 under a root
credential: 32 random bytes, written as 64 hex characters in `./secrets/bancwr_root_key`. Compose
mounts that file read-only into the bunker as a secret, and the bunker reads it, through
`BANCWR_ROOT_KEY_FILE`, at every start. The key file on its own, or a backup of `./data`, is no use
without the root credential.

The two files together are the key. Keep them apart: never put the root credential in a backup of
`./data`, or in the same place as one.

What this does not protect against: anyone who controls the host can read both files, or the
bunker's memory, and so the key. See [the threat model](threat-model.md#5-known-limits-and-accepted-risks).

### When the bunker will not sign

The bunker never reads a plain key while running. It starts locked, and signs nothing, when:

- `BUNKER_NSEC` or `BUNKER_NSEC_FILE` is still set, whether or not the key has been imported;
- `BANCWR_ROOT_KEY_FILE` is not set, or its file cannot be read or is not 64 hex characters;
- `./data/bunker-key.enc` is missing or cannot be read;
- the root credential does not decrypt the key file: either it is the wrong credential, or the key
  file has been altered.

A locked bunker keeps running so it can say why. The container shows as unhealthy, and the reason
appears in three places:

```bash
podman exec bancwr-bunker bunker healthcheck
podman logs bancwr-bunker
```

and on the console's dashboard, where the `signer` check is red with the same words. NIP-46 does
not start, and every console page except the dashboard's health answers with an error until you
fix the cause and restart the bunker.

### A new installation

1. Create the root credential, without it passing through your shell history:

   ```bash
   mkdir -m 700 secrets
   (umask 077; openssl rand -hex 32 > secrets/bancwr_root_key)
   ```

2. Put the bunker's key, as an nsec or in hex, in a file for the import. A trailing newline is fine.

   ```bash
   (umask 077; ${EDITOR:-nano} secrets/bunker_nsec)
   ```

   To make a new key instead, any Nostr key generator will do; keep its nsec offline, as
   [Keep copies somewhere else](#keep-copies-somewhere-else) says.

3. Hand both to the bunker's user, read-only:

   ```bash
   chmod 400 secrets/bancwr_root_key secrets/bunker_nsec
   mkdir -p data

   # Rootless Podman
   podman unshare chown -R 65532:65532 secrets data

   # Docker
   sudo chown -R 65532:65532 secrets data
   ```

4. Import the key, once. This mounts the plain key for this one command only:

   ```bash
   podman compose run --rm --no-deps \
     -v ./secrets/bunker_nsec:/run/secrets/bunker_nsec:ro,Z \
     -e BUNKER_NSEC_FILE=/run/secrets/bunker_nsec \
     bunker import-key
   ```

   With Docker, use `docker compose` in the same command. It prints the bunker's npub. It never
   replaces an existing `./data/bunker-key.enc`: to start again, stop the bunker and remove that
   file first.

5. Delete the plain key:

   ```bash
   # Rootless Podman
   podman unshare rm secrets/bunker_nsec

   # Docker
   sudo rm secrets/bunker_nsec
   ```

   Check that no other plain copy is left on the host: in `.env`, in any `compose.override.yaml`,
   in editor backups such as `bunker_nsec~`, or in your shell history.

6. Start, and check:

   ```bash
   podman compose up -d
   podman exec bancwr-bunker bunker healthcheck
   ```

   The bunker's log names the key it decrypted by its npub.

After step 3 your own user can no longer read the secrets. Use `podman unshare` or `sudo` to read
or replace them.

### Migrating from 0.1.0

A 0.1.0 bunker read its key in plain text from `BUNKER_NSEC` in `.env`, or from the file
`BUNKER_NSEC_FILE` names. After upgrading to 0.2.0 it does not sign until the key is imported and
the plain key is removed. Until then it runs locked and says so.

1. Stop the bunker:

   ```bash
   podman compose stop bunker
   ```

2. If the key is in `.env` as `BUNKER_NSEC`, put it in a file for the import first:

   ```bash
   mkdir -p -m 700 secrets
   (umask 077; ${EDITOR:-nano} secrets/bunker_nsec)
   chmod 400 secrets/bunker_nsec
   ```

   Paste the value of `BUNKER_NSEC` from `.env`, save and close. If you already used a secret file,
   it is `secrets/bunker_nsec` already.

3. Create the root credential, and hand both files to the bunker's user, as steps 1 and 3 of
   [A new installation](#a-new-installation) show.

4. Update `compose.yaml` to the 0.2.0 version. It no longer passes `BUNKER_NSEC`, and it mounts
   `./secrets/bancwr_root_key` as the `bancwr_root_key` secret. If you have a `compose.override.yaml`
   from the 0.1.0 secret-file steps, remove its `BUNKER_NSEC_FILE` line and its `bunker_nsec`
   secret.

5. Import the key, as step 4 of [A new installation](#a-new-installation) shows. Check that the
   npub it prints is the bunker's npub from before.

6. Remove every plain copy: delete `secrets/bunker_nsec` as in step 5 of
   [A new installation](#a-new-installation), delete the `BUNKER_NSEC` line from `.env`, and delete
   any other copy of `.env` that still holds it.

7. Start the bunker, and check it is healthy and has the same npub:

   ```bash
   podman compose up -d
   podman exec bancwr-bunker bunker healthcheck
   podman logs bancwr-bunker 2>&1 | grep 'Public key'
   ```

   If the health check says the plain key is still configured, step 4 or 6 left `BUNKER_NSEC` or
   `BUNKER_NSEC_FILE` set somewhere. Find it with:

   ```bash
   podman inspect bancwr-bunker --format '{{range .Config.Env}}{{println .}}{{end}}' | grep NSEC
   ```

The key was plain text on this host before. Encrypting it now does not undo any copy already made,
in a backup of `.env` or of the host. Nostr has no way to rotate or revoke a key, so if such a copy
may have left your control, the key should be treated as exposed.

### Keep copies somewhere else

If the host is lost, the key goes with it, and so does the bunker's identity. Keep one copy of the
plain key offline, in a password manager or on an encrypted drive.

Keep the root credential too, somewhere other than the backups of `./data`. Without it,
`bunker-key.enc` cannot be decrypted, and a restored bunker stays locked unless you import the
plain key again.

## Backing up the database

The bunker's database is SQLite, at `./data/bancwr.db`. It holds:

- the team: members and their roles;
- connection tokens (only a hash of each secret) and connected apps;
- the signing log;
- the bunker relays chosen under **Config**.

It does not hold the bunker's key, or anyone's session: sessions live in the frontend's memory.
The key is in its own file in the same directory, `./data/bunker-key.enc`, encrypted, so a copy of
the whole directory carries it as ciphertext only. The root credential is in `./secrets`, which is
never part of these backups.

The database runs in WAL mode. While the bunker is running, recent changes are in
`bancwr.db-wal`, not `bancwr.db`, and on a quiet bunker `bancwr.db` can be almost empty. Never copy
`bancwr.db` on its own from a running bunker. Use one of the two ways below.

The database names every member and every connected app, so keep backups as private as the host.
Set `./data` to `0700` too, as the restore below does.

### With the bunker stopped

The simplest copy, and the only one that needs nothing beyond compose. NIP-46 apps cannot sign
while the bunker is stopped.

```bash
podman compose stop bunker
mkdir -p -m 700 backups
mkdir -m 700 "backups/$(date +%F)"
podman unshare cp -a data/. "backups/$(date +%F)/"
podman compose start bunker
```

With Docker, use `docker compose` and `sudo cp -a` instead. When the bunker stops cleanly it folds
the WAL back into `bancwr.db`, so normally that is the only file there. Copying the whole directory
also takes any `-wal` and `-shm` file left by a crash, which belong with it.

### With the bunker running

SQLite's own `.backup` command makes one consistent file while the bunker keeps signing. The images
have no shell or `sqlite3`, so run it on the host, which needs the `sqlite3` command-line tool
installed.

```bash
mkdir -p -m 700 backups

# Rootless Podman
podman unshare sqlite3 data/bancwr.db ".backup 'backups/bancwr-$(date +%F).db'"

# Docker
sudo sqlite3 data/bancwr.db ".backup 'backups/bancwr-$(date +%F).db'"
```

Use this only while the bunker is running. Run against a stopped bunker's database as root, it can
leave `-wal` and `-shm` files owned by root, which the bunker then cannot open. When the bunker is
stopped, copy the file as above instead.

### Check the copy

```bash
sqlite3 backups/bancwr-2026-11-13.db "PRAGMA integrity_check"
```

It prints `ok`. Run it on the copy, never on `./data`. Use `podman unshare` or `sudo` in front if
your user cannot read the copy.

### Backups from before #42

Builds of the bunker before #42 could store the nsec in plain text in the database's `config`
table. Current builds delete it at start, but a backup made before that still contains it. Treat
such a backup as a copy of the key, or delete it.

## Restoring the database

1. Stop the bunker, and move the current data aside rather than deleting it:

   ```bash
   podman compose stop bunker
   mv data data.before-restore
   ```

2. Put the backup in a new `./data`, on its own. With a copy made while the bunker was stopped,
   copy every file in it; with a `.backup` file, copy only that, renamed to `bancwr.db`. Leave out
   any `-wal` or `-shm` file from the old data: they belong to the database you moved aside.

   A `.backup` file holds only the database, so copy the encrypted key back from the data you moved
   aside. Without it the bunker starts locked and says there is no encrypted key file.

   ```bash
   mkdir -m 700 data
   cp backups/bancwr-2026-11-13.db data/bancwr.db
   chmod 600 data/bancwr.db
   cp -p data.before-restore/bunker-key.enc data/

   # Rootless Podman
   podman unshare chown -R 65532:65532 data

   # Docker
   sudo chown -R 65532:65532 data
   ```

   With Docker, a backup made with `sudo` belongs to root: use `sudo` for the `cp` and `chmod` too.
   With rootless Podman, `data.before-restore` belongs to the bunker's user: use `podman unshare cp`
   for the key file.

3. Start the bunker and check it:

   ```bash
   podman compose start bunker
   podman exec bancwr-bunker bunker healthcheck
   ```

4. Before you leave the bunker running, check the team and the connections, as the next section
   explains.

Once you are sure the restore is good, delete `data.before-restore`.

### What a restore loses, and what it brings back

A restore puts the database back exactly as it was at the backup. Everything since is undone, in
both directions.

Lost:

- members added and roles changed since the backup;
- connection tokens issued since the backup, and apps connected since: each app needs a new token;
- signing log entries since the backup;
- changes to the bunker relays since the backup.

Brought back:

- members removed since the backup, with their roles;
- connections revoked since the backup, including those ended when their member was removed or
  made a viewer;
- connection tokens used or revoked since the backup, which can be used again until they expire.

That means a restore can give access back to people and apps you had shut out. Before you leave the
bunker running after a restore, sign in as an administrator and:

1. Under **Team**, remove anyone who should no longer be there, and correct any roles.
2. Under **Connections**, revoke every connected app you had revoked before, and every token under
   **Unused tokens** that you did not mean to leave open.

Sessions are not in the database, so a restore does not sign anyone out. Restarting the frontend
does, as it always does. The role of everyone already signed in is read from the restored
database on their next request.

0.2.0 replaces all of this with an encrypted backup of the database and key, and a restore that
revokes connections for you (#95).
