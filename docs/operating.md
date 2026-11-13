# Operating Bancwr: the key and the database

This page is for whoever runs a Bancwr bunker. It covers keeping the bunker's key out of the
environment, backing up the database, and restoring it. Until 0.2.0, Bancwr does none of this for
you: the key is not encrypted at rest (#94) and there is no built-in backup (#95).

Commands are given for rootless Podman and for Docker. They assume the `compose.yaml` layout from
the [README](../README.md#running-the-environment): the bunker's data in `./data` beside
`compose.yaml`, and both containers running as UID 65532.

## Protecting the bunker key

> The bunker's key is not encrypted at rest in 0.1.0. Anyone who can read it, from the secret
> file, `.env`, the container's environment or a backup of the host, holds the bunker's Nostr
> identity. Nostr has no way to rotate or revoke a key, so a copied key stays usable for good.
> Encryption at rest is #94.

### Use a secret file, not `BUNKER_NSEC`

`compose.yaml` passes the key as `BUNKER_NSEC`, from `.env`. A variable in the environment shows in
`podman inspect` or `docker inspect`, in `/proc/<pid>/environ` on the host, and in every copy of
`.env`. A file mounted as a compose secret shows in none of these.

The bunker reads `BUNKER_NSEC_FILE` before `BUNKER_NSEC`. The file holds the key, as an nsec or in
hex. A trailing newline is fine.

1. Create the file without the key reaching your shell history:

   ```bash
   mkdir -m 700 secrets
   (umask 077; ${EDITOR:-nano} secrets/bunker_nsec)
   ```

   Paste the key, save and close.

2. Make it read-only, and hand it to the bunker's user:

   ```bash
   chmod 400 secrets/bunker_nsec

   # Rootless Podman
   podman unshare chown -R 65532:65532 secrets

   # Docker
   sudo chown -R 65532:65532 secrets
   ```

   The directory stays at `0700` and the file at `0400`, owned by UID 65532. Compose mounts the
   file read-only into the container as it is on the host, so its owner and mode there decide who
   can read it. If the bunker cannot read the file it stops at once with
   `Could not read nsec from file: Permission denied`.

3. Add a `compose.override.yaml` beside `compose.yaml`. Compose reads it automatically.

   ```yaml
   services:
     bunker:
       environment:
         - BUNKER_NSEC_FILE=/run/secrets/bunker_nsec
       secrets:
         - bunker_nsec

   secrets:
     bunker_nsec:
       file: ./secrets/bunker_nsec
   ```

   If you already have a `compose.override.yaml` to run the `edge` images before the first release
   (see the README), add these lines to that file: keep its `image:` lines, and put the
   `environment:` and `secrets:` keys under the same `bunker:` service.

4. In `.env`, leave `BUNKER_NSEC` empty (`BUNKER_NSEC=`), and delete any other copy of `.env` that
   still holds the key.

5. Restart, and check:

   ```bash
   podman compose up -d
   podman exec bancwr-bunker bunker healthcheck
   podman inspect bancwr-bunker --format '{{range .Config.Env}}{{println .}}{{end}}' | grep NSEC
   ```

   With Docker, use `docker compose` and `docker` in the same commands. The last one should show
   `BUNKER_NSEC_FILE=/run/secrets/bunker_nsec` and an empty `BUNKER_NSEC=`. The bunker's log names
   the key it loaded by its npub.

After step 2 your own user can no longer read the file. To change the key later, use
`podman unshare` or `sudo` to edit it.

### Keep a copy of the key somewhere else

If the host is lost, the key goes with it, and so does the bunker's identity. Keep one copy offline,
in a password manager or on an encrypted drive. Do not store it with the database backups: they are
copied more often and to more places, and the database never needs the key to be restored.

## Backing up the database

The bunker's database is SQLite, at `./data/bancwr.db`. It holds:

- the team: members and their roles;
- connection tokens (only a hash of each secret) and connected apps;
- the signing log;
- the bunker relays chosen under **Config**.

It does not hold the bunker's key, or anyone's session: sessions live in the frontend's memory.

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

   ```bash
   mkdir -m 700 data
   cp backups/bancwr-2026-11-13.db data/bancwr.db
   chmod 600 data/bancwr.db

   # Rootless Podman
   podman unshare chown -R 65532:65532 data

   # Docker
   sudo chown -R 65532:65532 data
   ```

   With Docker, a backup made with `sudo` belongs to root: use `sudo` for the `cp` and `chmod` too.

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
