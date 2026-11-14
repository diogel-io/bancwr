# Operating Bancwr: the key and the database

This page is for whoever runs a Bancwr bunker. It covers the bunker's key, which is encrypted at
rest from 0.2.0 (#94), moving a 0.1.0 bunker to it, backing up and restoring the bunker with
`bunker backup` and `bunker restore` (#95), and keeping, exporting and pruning its signing log and
audit events (#96).

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

1. Create the root credential and the backup passphrase, without either passing through your
   shell history:

   ```bash
   mkdir -m 700 secrets
   (umask 077; openssl rand -hex 32 > secrets/bancwr_root_key)
   (umask 077; openssl rand -base64 24 > secrets/bancwr_backup_key)
   ```

   The backup passphrase encrypts backups (see [Backing up the bunker](#backing-up-the-bunker)).
   Compose mounts both files, so both must exist before any `podman compose` command. Copy the
   passphrase somewhere safe now, as [Keep copies somewhere else](#keep-copies-somewhere-else)
   says: without it, no backup can be restored.

2. Put the bunker's key, as an nsec or in hex, in a file for the import. A trailing newline is fine.

   ```bash
   (umask 077; ${EDITOR:-nano} secrets/bunker_nsec)
   ```

   To make a new key instead, any Nostr key generator will do; keep its nsec offline, as
   [Keep copies somewhere else](#keep-copies-somewhere-else) says.

3. Hand both to the bunker's user, read-only:

   ```bash
   chmod 400 secrets/bancwr_root_key secrets/bancwr_backup_key secrets/bunker_nsec
   mkdir -p -m 700 data

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

3. Create the root credential and the backup passphrase, and hand the files to the bunker's user,
   as steps 1 and 3 of [A new installation](#a-new-installation) show.

4. Update `compose.yaml` to the 0.2.0 version. It no longer passes `BUNKER_NSEC`, and it mounts
   `./secrets/bancwr_root_key` and `./secrets/bancwr_backup_key` as the `bancwr_root_key` and
   `bancwr_backup_key` secrets. If you have a `compose.override.yaml`
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

Keep the backup passphrase as well, apart from both the root credential and the backups. Without
it, a backup cannot be opened.

## Backing up the bunker

`bunker backup` writes one encrypted archive while the bunker keeps running. It holds:

- a consistent snapshot of the database, `./data/bancwr.db`: the team and their roles, connection
  tokens (only a hash of each secret) and connected apps, the signing log, and the bunker relays
  chosen under **Config**;
- the encrypted key file, `./data/bunker-key.enc`, exactly as it is on disk.

It does not hold the root credential, the backup passphrase, the proxy secret or anyone's session.
The archive is encrypted with XChaCha20-Poly1305 under a key derived with scrypt from the backup
passphrase in `./secrets/bancwr_backup_key`. Inside it, the bunker's key is still encrypted under
the root credential, so an archive and its passphrase together are not the key.

To recover a bunker you need three things, and each should be kept apart from the others:

1. an archive;
2. the backup passphrase, `./secrets/bancwr_backup_key`;
3. the root credential, `./secrets/bancwr_root_key`.

The database names every member and every connected app, so keep archives as private as the host.

### Take a backup

```bash
mkdir -p -m 700 backups
(umask 077; podman exec bancwr-bunker bunker backup > "backups/bancwr-$(date +%F).bcwr")
```

With Docker, use `docker exec`. The archive goes to standard output and a short summary to the
terminal: the bunker's npub, and how many members, connections and signing log entries it holds.
If the command fails, it exits non-zero and says why, and the file it was redirected to is empty or
incomplete: delete it.

The snapshot is taken with SQLite's `VACUUM INTO`, which briefly writes a plain copy of the database
beside it in `./data`, readable only by the bunker's user, and deletes it afterwards. That copy
never holds the key.

Bancwr does not copy archives anywhere. Copy them off the host yourself, to somewhere the backup
passphrase is not.

### Scheduled backups

A small script, a systemd timer to run it daily, and a 30-day retention. Adjust the directory to
where your `compose.yaml` lives. Save this as `bancwr-backup.sh` beside `compose.yaml` and make it
executable (`chmod 700 bancwr-backup.sh`):

```sh
#!/bin/sh
set -eu
umask 077
cd "$(dirname "$0")"
mkdir -p backups
name="backups/bancwr-$(date +%F-%H%M).bcwr"
podman exec bancwr-bunker bunker backup > "$name.part"
mv "$name.part" "$name"
find backups -name 'bancwr-*.bcwr' -mtime +30 -delete
```

With Docker, use `docker exec`. The archive only gets its final name once the backup succeeds, so a
failed run leaves a `.part` file and never an archive that looks complete.

With rootless Podman, run it as a systemd user timer. `~/.config/systemd/user/bancwr-backup.service`:

```ini
[Unit]
Description=Back up the Bancwr bunker

[Service]
Type=oneshot
ExecStart=%h/bancwr/bancwr-backup.sh
```

`~/.config/systemd/user/bancwr-backup.timer`:

```ini
[Unit]
Description=Back up the Bancwr bunker daily

[Timer]
OnCalendar=*-*-* 03:15:00
Persistent=true

[Install]
WantedBy=timers.target
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now bancwr-backup.timer
loginctl enable-linger "$USER"
```

`enable-linger` keeps the timer running while you are logged out. With Docker, put the same two
units in `/etc/systemd/system/` and use `systemctl` without `--user`. Check the last run with
`systemctl --user status bancwr-backup.service`.

Or with cron (`crontab -e`):

```text
15 3 * * * $HOME/bancwr/bancwr-backup.sh
```

Restore from one of these archives now and then, on a spare directory, to check you can: see the
next section.

## Restoring the bunker

`bunker restore` reads an archive from standard input. Before it writes anything it checks that:

- the backup passphrase decrypts the archive, which also proves the archive was not altered;
- the database in it passes SQLite's integrity check, and is not from a newer Bancwr;
- the key file in it matches the archive's public key;
- the root credential decrypts that key, so the restored bunker will unlock;
- the data directory holds no database and no key file.

If any check fails it exits non-zero, says why, and writes nothing. If they all pass, it writes the
database and key file, then revokes every connected app and every unused connection token. Members,
their roles, the signing log and the bunker relays come back as they were in the archive.

### On a clean host

1. Put `compose.yaml` and `.env` in place, as for a new installation, and restore the two secrets
   from where you kept them: `secrets/bancwr_root_key` and `secrets/bancwr_backup_key`. Give them,
   and an empty `./data`, to the bunker's user as step 3 of
   [A new installation](#a-new-installation) shows. There is no plain key to import.

2. Restore:

   ```bash
   podman compose run --rm --no-deps -T bunker restore < backups/bancwr-2026-11-13.bcwr
   ```

   With Docker, use `docker compose`. It prints the bunker's npub, how many members are back, and
   how many connections and tokens it revoked. Check the npub is the bunker's.

3. Start the bunker and check it:

   ```bash
   podman compose up -d
   podman exec bancwr-bunker bunker healthcheck
   ```

### Replacing a bunker's data

A restore never writes over existing data. To roll a bunker back, stop it and move its data aside
first, then restore into a new, empty `./data`:

```bash
podman compose stop bunker
mv data data.before-restore
mkdir -m 700 data

# Rootless Podman
podman unshare chown 65532:65532 data

# Docker
sudo chown 65532:65532 data
```

Then follow steps 2 and 3 above. Once you are sure the restore is good, delete `data.before-restore`.

### What a restore keeps, and what it revokes

Kept, as at the backup:

- members and their roles;
- the signing log and the audit events, to which the restore adds one saying what it revoked;
- the bunker relays and the log retention policy.

Revoked:

- every connected app, shown under **Connections** as ended because the bunker was restored from a
  backup. Each app needs a new connection token to connect again;
- every connection token not yet used.

Lost: anything since the backup, such as members added, roles changed and signatures made.

Members removed and roles lowered since the backup come back. Before you leave the bunker running,
sign in as an administrator and, under **Team**, remove anyone who should no longer be there and
correct any roles.

Sessions are not in the database, so a restore does not sign anyone out. Restarting the frontend
does, as it always does. The role of everyone already signed in is read from the restored database
on their next request.

## A manual copy

`bunker backup` is the supported way. If you cannot use it, copy `./data` with the bunker stopped.
NIP-46 apps cannot sign meanwhile.

```bash
podman compose stop bunker
mkdir -p -m 700 backups
mkdir -m 700 "backups/$(date +%F)"
podman unshare cp -a data/. "backups/$(date +%F)/"
podman compose start bunker
```

With Docker, use `docker compose` and `sudo cp -a` instead. Never copy `bancwr.db` on its own from a
running bunker: it runs in WAL mode, and recent changes are in `bancwr.db-wal`.

A manual copy is not encrypted apart from the key file, and restoring it is just copying it back.
That brings back every connection and token as it was, including those revoked since. After
restoring a manual copy, revoke under **Connections** every app and token you had shut out.

## The signing log and audit events

The bunker records every signature it makes, and every change to who may sign: connection tokens
issued and revoked, apps connected and disconnected, members added, removed and given another role,
the bunker relays, and the log's own retention policy (#96). Exports and pruning are recorded too.
Each change is recorded in the same database transaction as the change itself. Event content is
never recorded.

Administrators read the log under **Logs**, the newest 100 entries first, with **Load older** for
the rest. The page lists signatures. The other events are in the export.

### Exporting

Under **Logs**, **Export** downloads every signature and recorded change between two dates, as CSV
or JSON Lines. The dates are in UTC and both are included. Only administrators can export, and each
export is recorded with who made it and the range. `backend/README.md` describes every field and
event.

An exported file is outside Bancwr's control once downloaded. It holds members' and apps' public
keys and names and when they signed, but no keys that sign and no event content. Keep it as you
keep your other records of who acted for the organisation.

### Retention

By default the bunker keeps everything. Under **Config**, **Log retention**, an administrator can
keep entries for a number of days, keep at most a number of rows, or both. The two are applied to
signatures and to the other events separately. The shortest policy allowed is 30 days or 10,000
rows.

Saving a policy deletes what falls outside it at once. After that the bunker applies it at start
and every hour. Each change of policy, and each prune that deletes anything, is recorded with what
it deleted. Deleted entries are gone from the database and from every backup taken afterwards, so
export what you need to keep first. Earlier backups still hold them.

### Backups from before #42

Builds of the bunker before #42 could store the nsec in plain text in the database's `config`
table. Current builds delete it at start, but a backup made before that still contains it. Treat
such a backup as a copy of the key, or delete it.
