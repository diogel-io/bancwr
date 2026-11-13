// The bunker binary with its key encrypted at rest (#94): migrating a 0.1.0 deployment with
// `bunker import-key`, and the locked bunker refusing to sign, with `/health`, `bunker healthcheck`
// and the status saying why, for a plain key still set, a missing or wrong root credential and a
// tampered key file. Ends with a healthy start that signs a NIP-46 request with the imported key.
use bunker::health::{Check, CheckStatus, Overall, LOCKED_RELAYS};
use bunker::key_store::{KeyStoreError, PLAIN_KEY_NOT_IMPORTED, PLAIN_KEY_STILL_SET};
use bunker::proxy_auth::{proxy_signature, IDENTITY_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER};
use nostr_connect::prelude::*;
use nostr_relay_builder::MockRelay;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

const SECRET: &str = "bancwr-test-proxy-secret-0123456789abcdef";
const ROOT_HEX: &str = "8f1e2d3c4b5a69788796a5b4c3d2e1f00112233445566778899aabbccddeeff0";
const WRONG_HEX: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00112233445566778899aabbccddeeff0";

/// A deployment's files in a temporary directory, laid out as compose mounts them.
struct Deployment {
    dir: tempfile::TempDir,
    nsec: String,
    admin: Keys,
}

impl Deployment {
    fn new(bunker: &Keys) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        std::fs::create_dir(dir.path().join("secrets")).unwrap();
        let nsec = bunker.secret_key().to_bech32().unwrap();
        std::fs::write(dir.path().join("secrets/bunker_nsec"), format!("{}\n", nsec)).unwrap();
        std::fs::write(dir.path().join("secrets/root"), format!("{}\n", ROOT_HEX)).unwrap();
        std::fs::write(dir.path().join("secrets/wrong"), format!("{}\n", WRONG_HEX)).unwrap();
        Self { dir, nsec, admin: Keys::generate() }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn key_file(&self) -> PathBuf {
        self.path("data/bunker-key.enc")
    }

    /// The binary with only the given environment, run from the deployment's directory so no
    /// stray `.env` is read.
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

    fn import(&self, env: &[(&str, String)]) -> Output {
        self.command(&["import-key"], env).output().unwrap()
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A running `bunker serve`, killed when dropped.
struct Running {
    child: Child,
    port: u16,
    client: reqwest::Client,
    /// The seeded administrator, as hex.
    admin: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Running {
    async fn start(deployment: &Deployment, env: &[(&str, String)]) -> Running {
        let port = free_port();
        let mut env = env.to_vec();
        env.push(("BUNKER_PORT", port.to_string()));
        let child = deployment.command(&[], &env).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let running = Running { child, port, client: reqwest::Client::new(), admin: deployment.admin.public_key().to_hex() };
        for _ in 0..100 {
            if running.client.get(running.url("/health")).send().await.is_ok() {
                return running;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the bunker did not start listening");
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    fn signed(&self, method: reqwest::Method, path: &str, identity: &str) -> reqwest::RequestBuilder {
        let timestamp = chrono::Utc::now().timestamp();
        let signature = proxy_signature(SECRET.as_bytes(), timestamp, method.as_str(), path, identity);
        self.client
            .request(method, self.url(path))
            .header(IDENTITY_HEADER, identity)
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(SIGNATURE_HEADER, signature)
    }

    async fn health(&self) -> (u16, Value) {
        let res = self.client.get(self.url("/health")).send().await.unwrap();
        (res.status().as_u16(), res.json().await.unwrap())
    }

    /// The status as the frontend's service identity reads it.
    async fn status(&self) -> Value {
        let res = self.signed(reqwest::Method::GET, "/api/bunker/status", "service").send().await.unwrap();
        assert_eq!(res.status(), 200);
        res.json().await.unwrap()
    }

    /// `bunker healthcheck` against this bunker.
    fn probe(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bunker"))
            .arg("healthcheck")
            .env_clear()
            .env("BUNKER_PORT", self.port.to_string())
            .output()
            .unwrap()
    }

    /// Asserts the bunker is locked with `reason`, everywhere it can say so, and signs nothing.
    async fn assert_locked(&self, reason: &str, pubkey: &str) {
        let (code, body) = self.health().await;
        assert_eq!(code, 503);
        assert_eq!(body, json!({ "status": "locked", "reason": reason }));

        let probe = self.probe();
        assert_eq!(probe.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&probe.stderr).contains(reason), "{}", String::from_utf8_lossy(&probe.stderr));

        let status = self.status().await;
        assert_eq!(status["status"], json!(Overall::Unhealthy));
        assert_eq!(status["pubkey"], pubkey);
        let checks: Vec<Check> = serde_json::from_value(status["checks"].clone()).unwrap();
        let signer = checks.iter().find(|c| c.name == "signer").unwrap();
        assert_eq!((signer.status, signer.detail.as_str()), (CheckStatus::Fail, reason));
        let relays = checks.iter().find(|c| c.name == "relays").unwrap();
        assert_eq!((relays.status, relays.detail.as_str()), (CheckStatus::Fail, LOCKED_RELAYS), "NIP-46 never starts");

        // Nothing else answers, and no connection token can be issued.
        let admin = self.admin.clone();
        let res = self.signed(reqwest::Method::POST, "/api/bunker/connections/tokens", &admin).json(&json!({ "label": "x", "kinds": [1] })).send().await.unwrap();
        assert_eq!(res.status(), 503);
        assert_eq!(self.signed(reqwest::Method::GET, "/api/bunker/config", &admin).send().await.unwrap().status(), 503);
    }

}

fn root_env(deployment: &Deployment, file: &str) -> (&'static str, String) {
    ("BANCWR_ROOT_KEY_FILE", deployment.path(file).display().to_string())
}

fn assert_no_plain_key_in(dir: &Path, bunker: &Keys) {
    let nsec = bunker.secret_key().to_bech32().unwrap();
    let hex = bunker.secret_key().to_secret_hex();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let contents = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&contents);
        assert!(!text.contains(&nsec) && !text.contains(&hex), "{} holds the key", path.display());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_0_1_0_deployment_migrates_and_signs_with_the_encrypted_key() {
    let bunker = Keys::generate();
    let npub = bunker.public_key().to_bech32().unwrap();
    let deployment = Deployment::new(&bunker);
    let relay = MockRelay::run().await.unwrap();
    let nip46 = [("NIP46_ENABLED", "true".to_string()), ("NIP46_RELAYS", relay.url())];
    let plain_file = ("BUNKER_NSEC_FILE", deployment.path("secrets/bunker_nsec").display().to_string());
    let root = root_env(&deployment, "secrets/root");

    // 0.1.0 configuration, upgraded: locked, saying to import.
    {
        let env = [nip46[0].clone(), nip46[1].clone(), plain_file.clone(), root.clone()];
        Running::start(&deployment, &env).await.assert_locked(PLAIN_KEY_NOT_IMPORTED, "").await;
    }

    // Import needs the root credential.
    let output = deployment.import(&[plain_file.clone()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(KeyStoreError::RootKeyNotSet.reason()));
    assert!(!deployment.key_file().exists());

    // Import once: the npub is printed, the key never is.
    let output = deployment.import(&[plain_file.clone(), root.clone()]);
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&npub));
    assert!(!stdout.contains(&deployment.nsec));
    assert_no_plain_key_in(&deployment.path("data"), &bunker);

    // Never twice.
    let before = std::fs::read_to_string(deployment.key_file()).unwrap();
    let output = deployment.import(&[("BUNKER_NSEC", Keys::generate().secret_key().to_bech32().unwrap()), root.clone()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(KeyStoreError::KeyFileExists.reason()));
    assert_eq!(std::fs::read_to_string(deployment.key_file()).unwrap(), before);

    // Plain key still set, by file or by variable: locked, saying to remove it.
    for plain in [plain_file.clone(), ("BUNKER_NSEC", deployment.nsec.clone())] {
        let env = [nip46[0].clone(), nip46[1].clone(), plain, root.clone()];
        Running::start(&deployment, &env).await.assert_locked(PLAIN_KEY_STILL_SET, &npub).await;
    }

    // The operator removes the plain key. The bunker now holds the key only as ciphertext.
    std::fs::remove_file(deployment.path("secrets/bunker_nsec")).unwrap();
    assert_no_plain_key_in(&deployment.path("data"), &bunker);
    assert_no_plain_key_in(&deployment.path("secrets"), &bunker);

    // Healthy, as the imported key, and a standard NIP-46 client gets a note signed by it.
    let running = Running::start(&deployment, &[nip46[0].clone(), nip46[1].clone(), root.clone()]).await;
    assert_eq!(running.health().await, (200, json!({ "status": "ok" })));
    assert_eq!(running.probe().status.code(), Some(0));
    let admin = deployment.admin.public_key().to_hex();
    let config: Value = running.signed(reqwest::Method::GET, "/api/bunker/config", &admin).send().await.unwrap().json().await.unwrap();
    assert_eq!(config["pubkey"], npub);

    tokio::time::sleep(Duration::from_millis(500)).await; // let the relay client subscribe
    let res = running
        .signed(reqwest::Method::POST, "/api/bunker/connections/tokens", &admin)
        .json(&json!({ "label": "Migration test", "kinds": [1] }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let token: Value = res.json().await.unwrap();
    let client = NostrConnect::new(
        NostrConnectURI::parse(token["uri"].as_str().unwrap()).unwrap(),
        Keys::generate(),
        Duration::from_secs(10),
        None,
    )
    .unwrap();
    assert_eq!(client.get_public_key().await.unwrap(), bunker.public_key());
    let note = client.sign_event(EventBuilder::text_note("signed after #94").build(bunker.public_key())).await.unwrap();
    assert_eq!(note.pubkey, bunker.public_key());
    assert!(note.verify().is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_or_wrong_credential_or_a_tampered_file_locks_the_bunker() {
    let bunker = Keys::generate();
    let npub = bunker.public_key().to_bech32().unwrap();
    let deployment = Deployment::new(&bunker);
    let nip46 = [("NIP46_ENABLED", "true".to_string()), ("NIP46_RELAYS", "ws://127.0.0.1:1".to_string())];
    let plain_file = ("BUNKER_NSEC_FILE", deployment.path("secrets/bunker_nsec").display().to_string());
    let root = root_env(&deployment, "secrets/root");

    // No key file at all.
    {
        let env = [nip46[0].clone(), nip46[1].clone(), root.clone()];
        Running::start(&deployment, &env).await.assert_locked(KeyStoreError::KeyFileMissing.reason(), "").await;
    }

    assert_eq!(deployment.import(&[plain_file, root.clone()]).status.code(), Some(0));

    let cases: Vec<(Vec<(&str, String)>, &str)> = vec![
        (vec![], KeyStoreError::RootKeyNotSet.reason()),
        (vec![root_env(&deployment, "secrets/missing")], KeyStoreError::RootKeyUnreadable(std::io::Error::other("")).reason()),
        (vec![root_env(&deployment, "secrets/wrong")], KeyStoreError::WrongRootKeyOrTampered.reason()),
    ];
    for (extra, reason) in cases {
        let mut env = vec![nip46[0].clone(), nip46[1].clone()];
        env.extend(extra);
        Running::start(&deployment, &env).await.assert_locked(reason, &npub).await;
    }

    // One bit of the ciphertext changed.
    let text = std::fs::read_to_string(deployment.key_file()).unwrap();
    let mut fields: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    let last = fields[4].pop().unwrap();
    fields[4].push(if last == '0' { '1' } else { '0' });
    std::fs::write(deployment.key_file(), format!("{}\n", fields.join(" "))).unwrap();
    let env = [nip46[0].clone(), nip46[1].clone(), root];
    Running::start(&deployment, &env).await.assert_locked(KeyStoreError::WrongRootKeyOrTampered.reason(), &npub).await;
}
