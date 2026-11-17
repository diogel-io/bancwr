// Backup and restore through the binary (#95): `bunker backup` against a running bunker, with a
// signature in flight, then `bunker restore` onto a clean host. The restored bunker unlocks as the
// same key with the same members and roles, every connection and unused token is revoked, the old
// app cannot sign, and a new token connects. Every refusal writes nothing.
#[path = "../support/ports.rs"]
mod ports;

use bunker::backup::{BackupError, RESTORED_REASON};
use bunker::proxy_auth::{proxy_signature, IDENTITY_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER};
use nostr_connect::prelude::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

const SECRET: &str = "bancwr-test-proxy-secret-0123456789abcdef";
const ROOT_HEX: &str = "8f1e2d3c4b5a69788796a5b4c3d2e1f00112233445566778899aabbccddeeff0";
const WRONG_HEX: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00112233445566778899aabbccddeeff0";
const PASSPHRASE: &str = "a backup passphrase kept apart";

/// A host's deployment files in a temporary directory, laid out as compose mounts them.
struct Host {
    dir: tempfile::TempDir,
    admin: Keys,
}

impl Host {
    fn new(admin: &Keys) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("secrets")).unwrap();
        for (name, value) in [("root", ROOT_HEX), ("wrong_root", WRONG_HEX), ("backup", PASSPHRASE), ("wrong_backup", "not the backup passphrase")] {
            std::fs::write(dir.path().join("secrets").join(name), format!("{}\n", value)).unwrap();
        }
        Self { dir, admin: admin.clone() }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn secret(&self, name: &str) -> String {
        self.path(&format!("secrets/{}", name)).display().to_string()
    }

    /// The standard secrets: the right root credential and backup passphrase.
    fn secrets(&self) -> Vec<(&'static str, String)> {
        vec![("BANCWR_ROOT_KEY_FILE", self.secret("root")), ("BANCWR_BACKUP_KEY_FILE", self.secret("backup"))]
    }

    /// The binary with only the given environment, run from the host's directory so no stray
    /// `.env` is read.
    fn command(&self, args: &[&str], env: &[(&str, String)]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bunker"));
        command
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("DATABASE_PATH", self.path("data/bancwr.db"))
            .env("BANCWR_PROXY_SECRET", SECRET)
            .env("BANCWR_ADMIN_PUBKEY", self.admin.public_key().to_hex());
        for (name, value) in env {
            command.env(name, value);
        }
        command
    }

    fn import(&self, bunker: &Keys) {
        std::fs::create_dir(self.path("data")).unwrap();
        let nsec = bunker.secret_key().to_bech32().unwrap();
        let output = self.command(&["import-key"], &[("BUNKER_NSEC", nsec), ("BANCWR_ROOT_KEY_FILE", self.secret("root"))]).output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    }

    fn backup(&self, env: &[(&str, String)]) -> Output {
        self.command(&["backup"], env).stdin(Stdio::null()).output().unwrap()
    }

    fn restore(&self, archive: &[u8], env: &[(&str, String)]) -> Output {
        let mut child = self.command(&["restore"], env).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(archive).unwrap();
        child.wait_with_output().unwrap()
    }

    fn data_files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.path("data"))
            .map(|entries| entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }
}

/// A running `bunker serve`, killed when dropped.
struct Running {
    child: Child,
    port: u16,
    client: reqwest::Client,
    admin: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Running {
    /// `bunker serve` on a free port, started again on a new port if it exits before listening.
    async fn start(host: &Host, env: &[(&str, String)]) -> Running {
        let mut exits = Vec::new();
        for _ in 0..ports::ATTEMPTS {
            let port = ports::free_port();
            let mut env = env.to_vec();
            env.push(("BUNKER_PORT", port.to_string()));
            let child = host.command(&[], &env).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            let mut running = Running { child, port, client: reqwest::Client::new(), admin: host.admin.public_key().to_hex() };
            match running.listening().await {
                Ok(()) => {
                    // Let the relay client subscribe.
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    return running;
                }
                // Dropping it reaps the exited child.
                Err(status) => exits.push(format!("port {}: {}", port, status)),
            }
        }
        panic!("the bunker exited before listening on all {} attempts: {}", ports::ATTEMPTS, exits.join("; "));
    }

    /// Waits for `/health`. Err with the exit status if the child exits first, as it does when
    /// another socket took its port (#114): the locked bunker returns the bind error, the unlocked
    /// one logs it and exits 0. The child is checked again once `/health` answers, a cheap guard
    /// against taking another test's bunker on the same port for this one.
    async fn listening(&mut self) -> Result<(), std::process::ExitStatus> {
        for _ in 0..100 {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Err(status);
            }
            if self.client.get(self.url("/health")).send().await.is_ok() {
                return match self.child.try_wait().unwrap() {
                    Some(status) => Err(status),
                    None => Ok(()),
                };
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the bunker did not start listening on port {}", self.port);
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    fn signed(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let timestamp = chrono::Utc::now().timestamp();
        let signature = proxy_signature(SECRET.as_bytes(), timestamp, method.as_str(), path, &self.admin);
        self.client
            .request(method, self.url(path))
            .header(IDENTITY_HEADER, &self.admin)
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(SIGNATURE_HEADER, signature)
    }

    async fn get(&self, path: &str) -> Value {
        let res = self.signed(reqwest::Method::GET, path).send().await.unwrap();
        assert_eq!(res.status(), 200, "{}", path);
        res.json().await.unwrap()
    }

    async fn issue_token(&self) -> Value {
        let res = self.signed(reqwest::Method::POST, "/api/bunker/connections/tokens").json(&json!({ "label": "App", "kinds": [1] })).send().await.unwrap();
        assert_eq!(res.status(), 200);
        res.json().await.unwrap()
    }

    /// Members as (name, pubkey, role), sorted.
    async fn team(&self) -> Vec<(String, String, String)> {
        let team = self.get("/api/bunker/team").await;
        let mut members: Vec<(String, String, String)> = team
            .as_array()
            .unwrap()
            .iter()
            .map(|m| (m["name"].as_str().unwrap().into(), m["pubkey"].as_str().unwrap().into(), m["role"].as_str().unwrap().into()))
            .collect();
        members.sort();
        members
    }
}

fn connect(token: &Value, timeout: Duration) -> NostrConnect {
    NostrConnect::new(NostrConnectURI::parse(token["uri"].as_str().unwrap()).unwrap(), Keys::generate(), timeout, None).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_of_a_running_bunker_restores_to_a_working_bunker_on_a_clean_host() {
    let bunker = Keys::generate();
    let npub = bunker.public_key().to_bech32().unwrap();
    let admin = Keys::generate();
    let relay = ports::run_mock_relay().await;
    let nip46 = [("NIP46_ENABLED", "true".to_string()), ("NIP46_RELAYS", relay.url())];

    // The original host: a signer and a viewer beside the administrator, a connected app, and an
    // unused token.
    let original = Host::new(&admin);
    original.import(&bunker);
    let mut env = original.secrets();
    env.extend(nip46.clone());
    let running = Running::start(&original, &env).await;
    for (name, role) in [("Sam", "signer"), ("Vic", "viewer")] {
        let res = running
            .signed(reqwest::Method::POST, "/api/bunker/team")
            .json(&json!({ "name": name, "pubkey": Keys::generate().public_key().to_hex(), "role": role }))
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success(), "{}", res.status());
    }
    let app = connect(&running.issue_token().await, Duration::from_secs(5));
    assert_eq!(app.get_public_key().await.unwrap(), bunker.public_key());
    running.issue_token().await;
    let team_before = running.team().await;
    assert_eq!(team_before.len(), 3);

    // Back up while it runs, with a signature in flight.
    let signing = app.sign_event(EventBuilder::text_note("during the backup").build(bunker.public_key()));
    let backup = tokio::task::spawn_blocking({
        let command = original.command(&["backup"], &original.secrets());
        move || {
            let mut command = command;
            command.stdin(Stdio::null()).output().unwrap()
        }
    });
    let (signed, output) = tokio::join!(signing, backup);
    assert!(signed.unwrap().verify().is_ok());
    let output = output.unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stderr(&output).contains(&npub), "{}", stderr(&output));
    let archive = output.stdout;
    assert!(archive.starts_with(b"bancwr-backup v1 17 8 1 "));
    let text = String::from_utf8_lossy(&archive);
    assert!(!text.contains(&bunker.secret_key().to_bech32().unwrap()) && !text.contains(&bunker.secret_key().to_secret_hex()));
    assert!(original.data_files().iter().all(|name| !name.ends_with(".tmp")), "{:?}", original.data_files());

    // The host is lost.
    drop(running);
    drop(original);

    // A clean host: only its secrets, no data directory at all.
    let restored = Host::new(&admin);
    let output = restored.restore(&archive, &restored.secrets());
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&npub), "{}", stdout);
    assert!(stdout.contains("Revoked 1 connected apps and 1 unused connection tokens"), "{}", stdout);
    assert_eq!(restored.data_files(), vec!["bancwr.db", "bunker-key.enc"]);

    // A second restore over it is refused, and changes nothing.
    let before = std::fs::read(restored.path("data/bancwr.db")).unwrap();
    let output = restored.restore(&archive, &restored.secrets());
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains(BackupError::DataDirNotEmpty.reason()), "{}", stderr(&output));
    assert_eq!(std::fs::read(restored.path("data/bancwr.db")).unwrap(), before);

    // It starts unlocked as the same key, with the same members and roles.
    let mut env = restored.secrets();
    env.extend(nip46.clone());
    let running = Running::start(&restored, &env).await;
    let (code, health): (u16, Value) = {
        let res = running.client.get(running.url("/health")).send().await.unwrap();
        (res.status().as_u16(), res.json().await.unwrap())
    };
    assert_eq!((code, health), (200, json!({ "status": "ok" })));
    assert_eq!(running.get("/api/bunker/config").await["pubkey"], npub);
    assert_eq!(running.team().await, team_before);

    // Every connection and unused token is revoked.
    let connections = running.get("/api/bunker/connections").await;
    let connections = connections.as_array().unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0]["revoked_reason"], RESTORED_REASON);
    assert!(connections[0]["revoked_at"].is_string());
    let tokens = running.get("/api/bunker/connections/tokens").await;
    assert!(tokens.as_array().unwrap().iter().all(|t| t["used_at"].is_string() || t["revoked_at"].is_string()));

    // The old app cannot sign; a new token connects and signs.
    let refused = app.sign_event(EventBuilder::text_note("after the restore").build(bunker.public_key())).await;
    assert!(refused.is_err(), "a revoked connection signed");
    let fresh = connect(&running.issue_token().await, Duration::from_secs(10));
    let note = fresh.sign_event(EventBuilder::text_note("signed after #95").build(bunker.public_key())).await.unwrap();
    assert_eq!(note.pubkey, bunker.public_key());
    assert!(note.verify().is_ok());
}

/// An archive, the restore's environment, and the reason it must be refused with.
type Case<'a> = (Vec<u8>, Vec<(&'a str, String)>, &'a str);

#[tokio::test(flavor = "multi_thread")]
async fn a_tampered_or_wrong_key_archive_is_refused_and_writes_nothing() {
    let bunker = Keys::generate();
    let admin = Keys::generate();
    let original = Host::new(&admin);
    original.import(&bunker);
    // `bunker backup` needs no running bunker either: it reads the database file.
    {
        let mut env = original.secrets();
        env.push(("NIP46_ENABLED", "false".to_string()));
        let running = Running::start(&original, &env).await;
        drop(running);
    }
    let output = original.backup(&original.secrets());
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let archive = output.stdout;

    // The backup passphrase must be set, and must not be the root credential.
    let output = original.backup(&[("BANCWR_ROOT_KEY_FILE", original.secret("root"))]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains(BackupError::PassphraseNotSet.reason()));
    assert!(output.stdout.is_empty());
    let output = original.backup(&[("BANCWR_ROOT_KEY_FILE", original.secret("root")), ("BANCWR_BACKUP_KEY_FILE", original.secret("root"))]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains(BackupError::PassphraseIsRootKey.reason()));

    let target = Host::new(&admin);
    let mut tampered = archive.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    let cases: Vec<Case> = vec![
        (tampered, target.secrets(), BackupError::WrongPassphraseOrTampered.reason()),
        (
            archive.clone(),
            vec![("BANCWR_ROOT_KEY_FILE", target.secret("root")), ("BANCWR_BACKUP_KEY_FILE", target.secret("wrong_backup"))],
            BackupError::WrongPassphraseOrTampered.reason(),
        ),
        (
            archive.clone(),
            vec![("BANCWR_ROOT_KEY_FILE", target.secret("wrong_root")), ("BANCWR_BACKUP_KEY_FILE", target.secret("backup"))],
            BackupError::RootKeyDoesNotDecrypt.reason(),
        ),
        (archive[..archive.len() / 2].to_vec(), target.secrets(), BackupError::WrongPassphraseOrTampered.reason()),
        (b"not an archive\n".to_vec(), target.secrets(), BackupError::NotAnArchive.reason()),
    ];
    for (input, env, reason) in cases {
        let output = target.restore(&input, &env);
        assert_eq!(output.status.code(), Some(1), "{}", reason);
        assert!(stderr(&output).contains(reason), "expected {:?}, got {}", reason, stderr(&output));
        assert!(target.data_files().is_empty(), "{:?}", target.data_files());
    }

    // The same archive, with the right secrets, restores.
    let output = target.restore(&archive, &target.secrets());
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}


impl Host {
    /// `bunker import-key --add` (#103) with `key` and `args`.
    fn add_identity(&self, key: &Keys, args: &[&str]) -> Output {
        let nsec = key.secret_key().to_bech32().unwrap();
        let mut all = vec!["import-key", "--add"];
        all.extend_from_slice(args);
        self.command(&all, &[("BUNKER_NSEC", nsec), ("BANCWR_ROOT_KEY_FILE", self.secret("root"))]).output().unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_identity_added_at_the_command_line_signs_as_itself_and_survives_a_backup() {
    let bunker = Keys::generate();
    let brand = Keys::generate();
    let admin = Keys::generate();
    let relay = ports::run_mock_relay().await;
    let nip46 = [("NIP46_ENABLED", "true".to_string()), ("NIP46_RELAYS", relay.url())];

    let original = Host::new(&admin);
    original.import(&bunker);
    // Refused: no label, and the primary's key.
    assert_eq!(original.add_identity(&brand, &[]).status.code(), Some(1));
    let primary_again = original.add_identity(&bunker, &["--label", "Again"]);
    assert_eq!(primary_again.status.code(), Some(1));
    assert!(stderr(&primary_again).contains("already this bunker's primary identity"), "{}", stderr(&primary_again));

    // Added, with the key never printed.
    let added = original.add_identity(&brand, &["--label", "Brand B"]);
    assert_eq!(added.status.code(), Some(0), "{}", stderr(&added));
    let stdout = String::from_utf8_lossy(&added.stdout);
    assert!(stdout.contains(&brand.public_key().to_bech32().unwrap()), "{}", stdout);
    assert!(!stdout.contains(&brand.secret_key().to_bech32().unwrap()) && !stdout.contains(&brand.secret_key().to_secret_hex()));
    assert_eq!(original.add_identity(&brand, &["--label=Twice"]).status.code(), Some(1), "added once only");
    // At rest, only ciphertext.
    let db = std::fs::read(original.path("data/bancwr.db")).unwrap();
    let db = String::from_utf8_lossy(&db);
    assert!(!db.contains(&brand.secret_key().to_secret_hex()) && !db.contains(&brand.secret_key().to_bech32().unwrap()));

    let mut env = original.secrets();
    env.extend(nip46.clone());
    let running = Running::start(&original, &env).await;
    let identities = running.get("/api/bunker/identities").await;
    assert_eq!(identities.as_array().unwrap().len(), 2);
    assert_eq!(identities[1]["label"], "Brand B");
    assert_eq!(identities[1]["available"], true);

    // An administrator issues a connection for Brand B, and it signs as Brand B.
    let res = running
        .signed(reqwest::Method::POST, "/api/bunker/connections/tokens")
        .json(&json!({ "label": "Brand B app", "kinds": [1], "identity": brand.public_key().to_hex() }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let token: Value = res.json().await.unwrap();
    let app = connect(&token, Duration::from_secs(10));
    assert_eq!(app.get_public_key().await.unwrap(), brand.public_key());
    let note = app.sign_event(EventBuilder::text_note("from Brand B").build(brand.public_key())).await.unwrap();
    assert_eq!(note.pubkey, brand.public_key());

    // Backed up and restored onto a clean host, both identities unlock under the same credential.
    let output = original.backup(&original.secrets());
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let archive = output.stdout;
    drop(running);
    let restored = Host::new(&admin);
    let output = restored.restore(&archive, &restored.secrets());
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let mut env = restored.secrets();
    env.extend(nip46.clone());
    let running = Running::start(&restored, &env).await;
    let health: Value = running.client.get(running.url("/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(health, json!({ "status": "ok" }));
    let identities = running.get("/api/bunker/identities").await;
    assert_eq!(identities[1]["pubkey"], brand.public_key().to_hex());
    let res = running
        .signed(reqwest::Method::POST, "/api/bunker/connections/tokens")
        .json(&json!({ "label": "After the restore", "kinds": [1], "identity": brand.public_key().to_hex() }))
        .send()
        .await
        .unwrap();
    let fresh = connect(&res.json().await.unwrap(), Duration::from_secs(10));
    assert_eq!(fresh.sign_event(EventBuilder::text_note("restored").build(brand.public_key())).await.unwrap().pubkey, brand.public_key());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_identity_that_does_not_unlock_leaves_the_bunker_locked_naming_it() {
    let bunker = Keys::generate();
    let admin = Keys::generate();
    let host = Host::new(&admin);
    host.import(&bunker);
    assert_eq!(host.add_identity(&Keys::generate(), &["--label", "Brand B"]).status.code(), Some(0));
    // The identity's ciphertext altered in the database.
    let conn = rusqlite::Connection::open(host.path("data/bancwr.db")).unwrap();
    let key_file: String = conn.query_row("SELECT key_file FROM identities WHERE is_primary = 0", [], |row| row.get(0)).unwrap();
    let mut fields: Vec<String> = key_file.split_whitespace().map(str::to_string).collect();
    let last = fields.last_mut().unwrap();
    let flipped = if last.ends_with('0') { "1" } else { "0" };
    last.replace_range(last.len() - 1.., flipped);
    conn.execute("UPDATE identities SET key_file = ?1 WHERE is_primary = 0", [fields.join(" ")]).unwrap();
    drop(conn);

    let running = Running::start(&host, &host.secrets()).await;
    let res = running.client.get(running.url("/health")).send().await.unwrap();
    assert_eq!(res.status(), 503);
    let health: Value = res.json().await.unwrap();
    assert_eq!(health["status"], "locked");
    let reason = health["reason"].as_str().unwrap();
    assert!(reason.starts_with("The identity \"Brand B\" cannot be unlocked."), "{reason}");
}
