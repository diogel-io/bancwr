//! `nostrconnect://` codes (#148): an app's own offer to connect, which a member answers from the
//! console (NIP-46, "Direct connection initiated by the client").
//!
//! The code is `nostrconnect://<client-pubkey>?relay=…&secret=…`, with optional `perms`, `name`,
//! `url` and `image`. `relay` and `secret` are required: the bunker sends the `connect` response
//! to the client on those relays, with the secret as its result, and the client checks it.
//!
//! Everything in the code comes from the app and is checked here before anything is stored or
//! sent. The relays are normalised as the bunker's own are (`wss://`, or `ws://` on this machine
//! only), or are exactly one of the bunker's own relays, and at most `MAX_RELAYS` are kept. The
//! name and URL are unverified display hints, as `connect`'s metadata is (T10). The image is dropped: loading an address the app chose would
//! tell whoever runs it the operator's IP address and when the code was opened. The requested
//! permissions only pre-fill the console's form; the member chooses what is granted.
//!
//! rust-nostr's nostr-connect 0.39 writes an older form, with a `metadata` parameter and no
//! secret. It is refused, as NIP-46 now requires the secret.

use crate::bunker_relays::normalise_relay_url;
use crate::nip46::ENCRYPTION_METHODS;
use nostr::prelude::*;
use serde::Serialize;

pub const SCHEME: &str = "nostrconnect";
/// At most this many relays from one code: each may be a relay the bunker would not otherwise
/// use (B4), held open for the connection's life.
pub const MAX_RELAYS: usize = 3;
/// The longest code accepted, before parsing.
pub const MAX_URI_LENGTH: usize = 4096;
/// The longest secret accepted. NIP-46 suggests a short random string.
pub const MAX_SECRET_LENGTH: usize = 256;
/// The longest `perms` value accepted.
pub const MAX_PERMS_LENGTH: usize = 2048;
pub const MAX_NAME_CHARS: usize = 100;
pub const MAX_URL_CHARS: usize = 512;

/// What an app asked for in its `perms`: only a pre-fill for the console, and the bound on what
/// the member's choice can grant (`Requested::allows_kind`, `Requested::allows_method`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Requested {
    /// `sign_event` with no kind: any kind.
    pub any_kind: bool,
    /// `sign_event:<kind>`, sorted.
    pub kinds: Vec<u16>,
    /// Encryption methods (#97), in `ENCRYPTION_METHODS` order.
    pub methods: Vec<String>,
}

impl Requested {
    /// Whether the app asked for any permission the bunker grants. None: it asked for nothing, and
    /// gets what the member confirms.
    pub fn is_empty(&self) -> bool {
        !self.any_kind && self.kinds.is_empty() && self.methods.is_empty()
    }

    /// Whether the member's choice to sign `kind` stands: the app asked for nothing, or for this
    /// kind or any kind.
    pub fn allows_kind(&self, kind: u16) -> bool {
        self.is_empty() || self.any_kind || self.kinds.contains(&kind)
    }

    /// As `allows_kind`, for an encryption method.
    pub fn allows_method(&self, method: &str) -> bool {
        self.is_empty() || self.methods.iter().any(|m| m == method)
    }
}

/// A parsed, checked `nostrconnect://` code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NostrConnectUri {
    /// The app's NIP-46 key, which the `connect` response is addressed to.
    pub client_pubkey: PublicKey,
    /// Normalised, without repeats, in the code's order.
    pub relays: Vec<String>,
    /// Returned in the `connect` response, and never stored or logged.
    pub secret: String,
    pub requested: Requested,
    /// Unverified, trimmed and bounded.
    pub name: Option<String>,
    /// Unverified, `https://` or `http://` only.
    pub url: Option<String>,
}

/// Why a code was refused: a message the console shows as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl Refusal {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

/// Parses and checks `input`, a code an app showed.
pub fn parse(input: &str) -> Result<NostrConnectUri, Refusal> {
    parse_with(input, &[])
}

/// `parse`, where a relay may also be any of `bunker_relays`, the bunker's own relays in force,
/// however it is written: the bunker already uses it, so naming it exposes nothing new. That is
/// how an app on a compose network's relay, such as `ws://relay:7777`, names the relay the bunker
/// was given.
pub fn parse_with(input: &str, bunker_relays: &[String]) -> Result<NostrConnectUri, Refusal> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Refusal::new("Paste the app's nostrconnect:// code."));
    }
    if input.len() > MAX_URI_LENGTH {
        return Err(Refusal::new("That code is too long to be a nostrconnect:// code."));
    }
    let url = Url::parse(input).map_err(|_| Refusal::new("That is not a nostrconnect:// code."))?;
    if url.scheme() != SCHEME {
        return Err(Refusal::new(if url.scheme() == "bunker" {
            "That is a bunker:// link, which a signer gives an app. Paste the nostrconnect:// code the app shows."
        } else {
            "That is not a nostrconnect:// code."
        }));
    }
    let host = url.host_str().unwrap_or("");
    if host.len() != 64 || !host.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Refusal::new("The code does not name the app's key: it should follow nostrconnect:// as 64 hex characters."));
    }
    let client_pubkey = PublicKey::from_hex(host).map_err(|_| Refusal::new("The app's key in the code is not a valid Nostr key."))?;

    let mut relays: Vec<String> = Vec::new();
    let mut secret: Option<String> = None;
    let mut perms: Option<String> = None;
    let mut name: Option<String> = None;
    let mut link: Option<String> = None;
    let mut legacy_metadata = false;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "relay" => {
                let relay = match normalise_relay_url(&value) {
                    Ok(relay) => relay,
                    Err(reason) => bunker_relay(&value, bunker_relays).ok_or_else(|| Refusal::new(format!("{} is not a relay the bunker can use: {}", value.trim(), reason)))?,
                };
                if !relays.contains(&relay) {
                    relays.push(relay);
                }
            }
            "secret" => secret = Some(value.into_owned()),
            "perms" => perms = Some(value.into_owned()),
            "name" => name = Some(value.into_owned()),
            "url" => link = Some(value.into_owned()),
            "metadata" => legacy_metadata = true,
            // `image` is dropped (see the module comment), and anything else is not NIP-46's.
            _ => {}
        }
    }

    if relays.is_empty() {
        return Err(Refusal::new("The code names no relay, so the bunker cannot reach the app."));
    }
    if relays.len() > MAX_RELAYS {
        return Err(Refusal::new(format!("The code names more than {} relays; the bunker uses at most {} for one app.", MAX_RELAYS, MAX_RELAYS)));
    }
    let secret = secret.map(|s| s.trim().to_string()).unwrap_or_default();
    if secret.is_empty() {
        return Err(Refusal::new(if legacy_metadata {
            "The code has no secret: the app uses an older form of nostrconnect:// that NIP-46 no longer allows. Use a bunker:// link with this app instead."
        } else {
            "The code has no secret, which NIP-46 requires so the app can tell its own signer's answer from anyone else's."
        }));
    }
    if secret.len() > MAX_SECRET_LENGTH || secret.chars().any(char::is_control) {
        return Err(Refusal::new("The code's secret is not one the bunker can return."));
    }
    let requested = match perms {
        Some(perms) if perms.len() > MAX_PERMS_LENGTH => return Err(Refusal::new("The code asks for too many permissions to read.")),
        Some(perms) => parse_perms(&perms),
        None => Requested::default(),
    };

    Ok(NostrConnectUri {
        client_pubkey,
        relays,
        secret,
        requested,
        name: display(name, MAX_NAME_CHARS),
        url: display(link, MAX_URL_CHARS).filter(|v| v.starts_with("https://") || v.starts_with("http://")),
    })
}

/// The one of `bunker_relays` that `value` names, written as the bunker has it, ignoring a trailing
/// slash and the case of the scheme and host.
fn bunker_relay(value: &str, bunker_relays: &[String]) -> Option<String> {
    let comparable = |url: &str| Url::parse(url.trim()).ok().map(|u| u.as_str().trim_end_matches('/').to_string());
    let wanted = comparable(value)?;
    bunker_relays.iter().find(|own| comparable(own).as_deref() == Some(wanted.as_str())).cloned()
}

/// NIP-46 permissions, `method[:params]` separated by commas, keeping those the bunker grants.
/// Anything else (`get_public_key`, `ping`, unknown methods, bad kinds) is ignored: they decide
/// nothing.
fn parse_perms(perms: &str) -> Requested {
    let mut requested = Requested::default();
    for perm in perms.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if perm == "sign_event" {
            requested.any_kind = true;
        } else if let Some(kind) = perm.strip_prefix("sign_event:").and_then(|k| k.trim().parse::<u16>().ok()) {
            if !requested.kinds.contains(&kind) {
                requested.kinds.push(kind);
            }
        } else if ENCRYPTION_METHODS.contains(&perm) && !requested.methods.iter().any(|m| m == perm) {
            requested.methods.push(perm.to_string());
        }
    }
    requested.kinds.sort_unstable();
    requested.methods = ENCRYPTION_METHODS.iter().filter(|m| requested.methods.iter().any(|r| r == *m)).map(|m| m.to_string()).collect();
    requested
}

/// Unverified text for display: trimmed, without control characters, at most `max` characters,
/// None when nothing is left.
fn display(value: Option<String>, max: usize) -> Option<String> {
    let value: String = value?.chars().filter(|c| !c.is_control()).collect();
    let value = value.trim();
    (!value.is_empty()).then(|| value.chars().take(max).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "79dff8f82963424e0bb02708a22e44b4980893e3a4be0fa3cb60a43b946764e3";

    fn code(query: &str) -> String {
        format!("nostrconnect://{}?{}", KEY, query)
    }

    #[test]
    fn a_nip46_code_is_read_with_its_relays_secret_perms_and_metadata() {
        let parsed = parse(&code(
            "relay=wss%3A%2F%2FRelay.Example.com%2F&relay=wss://relay.example.com&relay=ws://localhost:7777&secret=0s8j2djs&perms=sign_event:7,nip44_encrypt,sign_event:1,get_public_key,sign_event:x&name=My%20Client&url=https://client.example.com&image=https://client.example.com/logo.png",
        ))
        .unwrap();
        assert_eq!(parsed.client_pubkey.to_hex(), KEY);
        assert_eq!(parsed.relays, vec!["wss://relay.example.com".to_string(), "ws://localhost:7777".to_string()]);
        assert_eq!(parsed.secret, "0s8j2djs");
        assert_eq!(parsed.requested, Requested { any_kind: false, kinds: vec![1, 7], methods: vec!["nip44_encrypt".to_string()] });
        assert_eq!(parsed.name.as_deref(), Some("My Client"));
        assert_eq!(parsed.url.as_deref(), Some("https://client.example.com"));
    }

    #[test]
    fn a_code_without_perms_asks_for_nothing_and_bare_sign_event_asks_for_any_kind() {
        let plain = parse(&code("relay=wss://r.example&secret=s")).unwrap();
        assert!(plain.requested.is_empty());
        assert!(plain.requested.allows_kind(30023) && plain.requested.allows_method("nip04_decrypt"));

        let any = parse(&code("relay=wss://r.example&secret=s&perms=sign_event")).unwrap();
        assert!(any.requested.allows_kind(1) && any.requested.allows_kind(30023));
        assert!(!any.requested.allows_method("nip44_encrypt"));

        let some = parse(&code("relay=wss://r.example&secret=s&perms=sign_event:1")).unwrap();
        assert!(some.requested.allows_kind(1) && !some.requested.allows_kind(7));
    }

    fn refused(input: &str) -> String {
        parse(input).expect_err("refused").0
    }

    #[test]
    fn malformed_codes_are_refused_with_a_reason() {
        assert!(refused("").contains("Paste"));
        assert!(refused("hello").contains("not a nostrconnect://"));
        assert!(refused(&format!("bunker://{}?relay=wss://r.example&secret=s", KEY)).contains("bunker:// link"));
        assert!(refused("nostrconnect://abc?relay=wss://r.example&secret=s").contains("64 hex"));
        assert!(refused(&code("secret=s")).contains("no relay"));
        assert!(refused(&code("relay=ws://relay.example.com&secret=s")).contains("unencrypted ws://"));
        assert!(refused(&code("relay=https://relay.example.com&secret=s")).contains("wss://"));
        assert!(refused(&code("relay=wss://a.example&relay=wss://b.example&relay=wss://c.example&relay=wss://d.example&secret=s")).contains("more than 3"));
        assert!(refused(&code(&format!("relay=wss://r.example&secret={}", "s".repeat(MAX_SECRET_LENGTH + 1)))).contains("secret"));
        assert!(refused(&code(&format!("relay=wss://r.example&secret=s&perms={}", "a".repeat(MAX_PERMS_LENGTH + 1)))).contains("too many"));
        assert!(refused(&format!("{}{}", code("relay=wss://r.example&secret=s&x="), "a".repeat(MAX_URI_LENGTH))).contains("too long"));
    }

    #[test]
    fn a_code_without_a_secret_is_refused_and_the_older_form_says_why() {
        assert!(refused(&code("relay=wss://r.example")).contains("no secret"));
        assert!(refused(&code("relay=wss://r.example&secret=%20%20")).contains("no secret"));
        let legacy = refused(&code("metadata=%7B%22name%22%3A%22App%22%7D&relay=wss://r.example"));
        assert!(legacy.contains("older form"), "{}", legacy);
    }

    #[test]
    fn a_relay_the_bunker_already_uses_may_be_any_address() {
        let own = vec!["ws://relay:7777".to_string()];
        let parsed = parse_with(&code("relay=ws%3A%2F%2FRELAY%3A7777%2F&secret=s"), &own).unwrap();
        assert_eq!(parsed.relays, own);
        assert!(parse(&code("relay=ws%3A%2F%2Frelay%3A7777&secret=s")).is_err());
        assert!(parse_with(&code("relay=ws%3A%2F%2Fother%3A7777&secret=s"), &own).is_err());
    }

    #[test]
    fn metadata_is_bounded_and_the_url_must_be_a_web_address() {
        let parsed = parse(&code(&format!("relay=wss://r.example&secret=s&name=%20{}%0A&url=javascript:alert(1)", "n".repeat(300)))).unwrap();
        assert_eq!(parsed.name.unwrap().chars().count(), MAX_NAME_CHARS);
        assert_eq!(parsed.url, None);
        let blank = parse(&code("relay=wss://r.example&secret=s&name=%20%20")).unwrap();
        assert_eq!(blank.name, None);
    }
}
