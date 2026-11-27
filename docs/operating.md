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
   mkdir -p -m 700 data sessions

   # Rootless Podman
   podman unshare chown -R 65532:65532 secrets data sessions

   # Docker
   sudo chown -R 65532:65532 secrets data sessions
   ```

   `sessions` is the frontend's, for live sign-in sessions (see [Sign-in sessions](#sign-in-sessions)).
   The frontend refuses to start if it cannot write there.

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

## Sign-in sessions

The frontend keeps live sign-in sessions in `./sessions` (#100), so restarting or upgrading it
signs nobody out. A session still ends after 12 hours, or when its member signs out, which removes
it at once, before the frontend answers, so a copy of the cookie stops working too.

Each session is one small file, named by the SHA-256 of its id. The id itself is never written,
and the cookie that carries it is sealed with `NUXT_SESSION_PASSWORD`, so a copy of the directory
is no way to sign in. It holds the signed-in member's public key and when the session ends.

- **Backups.** `./sessions` is not part of a backup and needs none. A lost or emptied directory
  only means signing in again.
- **Sign everyone out.** Stop the frontend, empty the directory, and start it again:

  ```bash
  podman compose stop frontend
  find sessions -type f -delete     # with Docker: sudo find sessions -type f -delete
  podman compose start frontend
  ```

  With rootless Podman, if the files are not yours on the host, run the `find` under
  `podman unshare`.
- **`NUXT_SESSION_PASSWORD`.** Changing it also signs everyone out: no earlier cookie can be read.

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

The archive is also authenticated under the root credential: it carries an HMAC-SHA256 tag over
its header and contents, keyed from the root credential. So `bunker backup` needs both secrets.
Run in the bunker's container, as below, it has them already. It checks that the root credential
decrypts the key file before it writes anything.

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

- the backup passphrase decrypts the archive;
- the key file in it matches the archive's public key;
- the root credential decrypts that key, so the restored bunker will unlock;
- the archive's tag verifies under the root credential, which proves the archive was made under
  it and not altered since;
- the database in it passes SQLite's integrity check, is not from a newer Bancwr, and holds only
  tables and indexes: no triggers, views or virtual tables, which a Bancwr database never has;
- the data directory holds no database and no key file.

So a restore needs the same root credential that made the backup. That was already the case: the
root credential has to decrypt the archive's key file, which is encrypted under one root credential.

Archives are in format version 2. A version 1 archive, made before archives were authenticated
under the root credential, is refused: take a new backup.

If any check fails it exits non-zero, says why, and writes nothing. If they all pass, it revokes
every connected app and every unused connection token in a staged copy of the database, and records
the restore there, before it puts the database and key file into the data directory. So if a
restore fails partway, the data directory is left empty, never holding a database whose apps and
tokens still work. Members, their roles, the signing log and the bunker relays come back as they
were in the archive.

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
- the signing log and the audit events, to which the restore adds one saying what it revoked and
  when the backup was taken;
- the bunker relays and the log retention policy.

Revoked:

- every connected app, shown under **Connections** as ended because the bunker was restored from a
  backup. Each app needs a new connection token to connect again;
- every connection token not yet used.

Lost: anything since the backup, such as members added, roles changed and signatures made.

Members removed and roles lowered since the backup come back. Before you leave the bunker running,
sign in as an administrator and, under **Team**, remove anyone who should no longer be there and
set any role back with the member's role select. A role change revokes nothing, because both roles
sign: to end a connection a new token gave someone since the restore, remove them or revoke it under
**Connections**.

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

## Signers connecting their own apps

A signer connects their own apps under **Connections**, without asking an administrator (#142).
Their form issues a token only for them, and offers only the identities they are granted. They see
and revoke their own unused tokens there too.

- A signer's token never needs approval: **Needs approval** is offered only to administrators. To
  have a member's app checked before it signs, issue the token for them yourself, with the kinds
  ticked.
- A signer can issue a new token after you revoke one. To stop a member connecting apps at all,
  remove them, which also disconnects their apps. Every member can sign (#143).
- Each token records who issued it, and the audit log names them.

## Apps with a time limit

A contractor, a campaign or a bot often needs access for a set time. When issuing a token under
**Connections**, choose how long the connection lasts under **Connection lasts**: 1, 7, 30 or 90
days, or **Until revoked**, which is the default and how every earlier connection behaves (#98).

- The time counts from when the app connects, not from when the token is issued. **Valid for** is
  separate: it is how long the token can be used to connect.
- When the time runs out, the bunker refuses the app and marks the connection ended, with the
  reason "Its time limit ran out". The audit log records it as `connection_revoked`,
  `reason=expired`.
- Each connection shows when it ends, or "no time limit".
- A time limit cannot be extended. To give more time, issue a new token, and the app connects again.
- You can still revoke a connection before its time runs out.

## How fast an app may sign

Every connection is rate limited (#101), so a runaway bot or a compromised app cannot sign or
decrypt without limit. When issuing a token under **Connections**, choose under **Rate limit**: the
default is 60 signing and encryption requests a minute with a burst of 20, and the presets run from
10 to 600 a minute.

- Past its limit, the app is refused and told how many seconds to wait. Normal use never comes near
  the default: it is for a loop that has gone wrong.
- The dashboard's **Requests refused (rate limit)** counts refusals since the bunker started. The
  bunker's log has a warning, and the audit log a `connection_rate_limited` event, at most once a
  minute for each connection that keeps being refused. A connection that shows up there again and
  again is worth a look: revoke it if it should not be doing that.
- Each token and connection shows its limit. A limit cannot be changed: issue a new token.
- Apps connected before the upgrade are limited to the default.

## Apps that need approval

A brand often wants a person to check what a bot or a member's app posts before it goes out (#102).
When issuing a token under **Connections**, tick the kinds under **Needs approval**: each request
to sign one of them waits under **Approvals** until an administrator, or a lead of the department
whose identity the app signs as (#147), decides.

- Each request shows the note or event exactly as it will be signed, who it is for and which app
  asked. **Approve and sign** signs it and sends it to the app, which publishes it. **Reject** signs
  nothing and tells the app why, with your note if you give one. Either is recorded in the audit
  log with who decided.
- A request nobody decides times out after 30 minutes, or `BANCWR_APPROVAL_TIMEOUT_MINUTES` (1 to
  1,440) in `.env`, and nothing is signed. The dashboard's **Waiting for approval** shows how many
  are waiting.
- Apps differ in how long they wait. An app that gives up sooner never receives the signed event,
  even if you approve it later: approve promptly, or set the app's own timeout longer if it has one.
- Requests waiting are lost when the bunker restarts, and their apps get no answer. They are kept
  only in memory so their content is never written to disk.
- Signers see their own requests waiting, and cannot decide them. Any administrator can, and a lead
  can for their department, their own apps' requests included. The audit log marks an approval by
  the app's own member with `self=true`.
- Which kinds need approval cannot be changed on an existing connection: issue a new token.
- Only an administrator, or a lead for their department, can require approval. Other signers' own
  tokens never need it (#142).

## More than one identity

One bunker can sign as several Nostr keys (#103), such as a company account and one for each
department. The key you imported first is the **primary**.

### Creating an identity in the console

1. Under **Config**, **Identities**, type a label in **Create an identity**, such as "Sales", and
   select **Create identity**. The bunker generates the key, encrypts it under the root credential
   and can sign as it at once. No restart is needed.
2. Take a backup straight away (see "The key and backups"). The key is never shown and can't be
   exported: if you lose this bunker's database, every backup or the root credential, the
   identity is gone for good.
3. Put members in its department and issue tokens for it, as in steps 3 and 4 below. A lead who
   creates an identity leads its new department.

If the bunker can't read its root credential (`BANCWR_ROOT_KEY_FILE`), creating is refused and
nothing changes. The bunker keeps signing as before.

### Importing a key you already have

1. Put the new key in a file the bunker's container can read, as you did for the first, and add it:

   ```bash
   podman compose run --rm -e BUNKER_NSEC_FILE=/run/secrets/brand_b bunker import-key --add --label "Brand B"
   ```

   It prints the new identity's npub, never its key. Then delete every plain copy of the key, as
   after the first import.
2. Restart the bunker: a new identity loads at start. Until then, **Config**, **Identities** shows it
   as "Loads when the bunker restarts".
3. Put members in the departments they sign for, on their team page under **Departments**, or on
   the **Departments** page. Administrators may sign as every identity. When you add the first
   identity beside the primary, every member who could sign keeps the primary.
4. Issue connection tokens for the identity under **Connections**, **Sign as**.

- Taking a member out of a department disconnects their apps for that identity, and revokes their
  unused tokens for it. Their other departments are untouched.
- Every identity is encrypted under the same root credential, and every backup carries them all.
  Restoring needs that credential, as it always has.
- If an identity cannot be decrypted at start (its row altered, or the wrong credential), the
  bunker stays locked and its status names the identity by its label. Nothing signs until it is
  fixed: restore the database from a backup.
### Editing an identity's profile, follows and relays

Each identity is its own Nostr account, with its own name, picture, follows and relays (#150).
Under **Config**, **Identities**, select **Profile**, **Follows** or **Relays** beside it. These are
the same pages as your own, with a banner naming the identity, such as "Editing Sales":

- The bunker signs each change as the identity. You don't need its key, and your own signer isn't
  asked.
- Changes go to the identity's own NIP-65 relays and the profile relays, as for your own account.
  The bunker's relays carry NIP-46 traffic only, so they are left out.
- A profile picture is uploaded to the identity's Blossom server, or the default one.
- The signing log records each change under your name, with **Console** as the app.
- Administrators can do this for every identity, and a lead for their department's, from
  **Departments**. An identity that loads only after a restart has no links until then.

### Retiring an identity

When an account is no longer needed, select **Retire** beside it under **Config**, **Identities**.
The confirmation says how many connected apps and unused tokens it revokes. Retiring:

- disconnects every app signing as it and revokes its unused tokens,
- removes every member's access to it,
- destroys its encrypted key, so nothing can sign as it again,
- keeps its signatures and audit events in the log and the export, still under its label.

The primary can't be retired. A restore from a backup taken before you retired an identity brings
it back, with its key, as it brings back a member removed since that backup. Its apps still need
new tokens, as after every restore.

## Departments

Each identity is a department (#147), such as Sales or Support, used by its members. A person can be
in any number of departments. **Departments** lists each one with its members and its leads.

- **Leads.** An administrator makes someone a lead with **Make lead** on **Departments**, or with
  their role in that department on their team page. A lead adds and removes their department's
  members, registers new people's keys into it as signers, issues its connections with approval
  kinds, approves its requests, edits its identity's profile, and can create a new identity, which
  they then lead, or retire their own. A lead can't touch another department, change anyone's role,
  remove anyone from the bunker, or open Config or Logs. A person a lead registered stays in the
  bunker after leaving every department, until an administrator removes them.
- **Making a lead a member** keeps them in the department, with their apps. Taking someone out of a
  department disconnects their apps for it.
- **Administrator only.** An identity can belong to no department, such as the company root: only
  administrators sign as it, manage its connections and approve its requests, and nobody can be
  added to it. Select **Make administrator only** on its card once nobody is in it; the bunker
  refuses while anyone but an administrator may still use it. **Make it a department** undoes it.
- **The company root's own bunker** (several bunkers ADR): import its key administrator only, so
  no member is ever granted it:

  ```bash
  podman compose run --rm --no-deps \
    -v ./secrets/bunker_nsec:/run/secrets/bunker_nsec:ro,Z \
    -e BUNKER_NSEC_FILE=/run/secrets/bunker_nsec \
    bunker import-key --admin-only
  ```

  `import-key --add --admin-only --label "Holding"` does the same for an identity added beside the
  primary.
- **Upgrading needs no step of yours.** Every existing grant becomes a department membership, nobody
  becomes a lead and nothing becomes administrator only, so nobody gains or loses access. The audit
  log records one `department_migrated` event per grant. A bunker with only its primary records
  none, and behaves as before until a lead is appointed or another identity is added.
- Every change, who made it, and every approval are in the audit log and the export.

## Apps that read and send private messages

An app can be allowed to encrypt and decrypt with the brand's key (#97), which direct messages
(NIP-17), private lists and some other apps need. Under **Connections**, when issuing a token,
tick what it may also do: **Encrypt messages** and **Read encrypted messages**, for NIP-44, and the
same pair for NIP-04, which only older apps use. Each is a separate permission. Choose **Nothing:
encryption only** under **May sign** for an app that does not sign.

Before allowing an app to read encrypted messages:

- It can read every encrypted message sent to the brand's key, from anyone. There is no way to
  limit it to particular senders.
- Each decrypt is recorded with the member, the app and the sender's key, but not the message. They
  are in the export, as `nip44_decrypt` and `nip04_decrypt`.
- A direct-message inbox decrypts twice for each message, so it adds audit events quickly. Set a
  retention policy if the log should not grow without limit.
- Revoking the app's connection stops it at once.

No app can encrypt or decrypt with the key of another app that has connected to the bunker, or
that has sent it any request, even one that was refused. Nor does a decrypt ever return a NIP-46
request or response, whichever key it names (`Forbidden: <method> does not return NIP-46
messages`). That protects what apps and the bunker send each other, including a request the bunker
never received because it was stopped.

A connection token that an app presents but that is refused, for example because it is for another
identity or the app asks only for permissions the token does not allow, is used up: it shows as
used, with no connection, and the audit log has a `token_revoked` event with
`reason=connect_refused`. Issue a new token.

## The signing log and audit events

The bunker records every signature it makes, and every change to who may sign: connection tokens
issued and revoked, apps connected and disconnected, members added, removed and given another role,
the bunker relays, and the log's own retention policy (#96). Exports and pruning are recorded too.
Each change is recorded in the same database transaction as the change itself. Every decrypt an app
asks for is recorded before it happens (#97). Event content and message content are never recorded.

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
