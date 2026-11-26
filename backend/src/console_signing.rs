//! What the console may have an identity sign (#150), checked before anything is signed.
//!
//! An administrator edits an identity's profile (kind 0), follow list (kind 3) and NIP-65 relay
//! list (kind 10002) in the console, and uploads its profile image to a Blossom server with a
//! BUD-02 upload authorisation (kind 24242). The console builds each event in the browser and the
//! bunker signs it as the identity through `POST /api/bunker/identities/:pubkey/sign`. This is the
//! only route that signs on request, and it signs nothing else: not a note, not a DM, not a NIP-98
//! sign-in, not a deletion. There is still no route that signs any kind (#25).
//!
//! The checks are here, apart from the route, so they can be tested on their own.
use nostr::prelude::*;
use serde::Deserialize;

/// The kinds the console may have an identity sign.
pub const PROFILE_KIND: u16 = 0;
pub const FOLLOWS_KIND: u16 = 3;
pub const RELAY_LIST_KIND: u16 = 10002;
pub const BLOSSOM_AUTH_KIND: u16 = 24242;
pub const CONSOLE_KINDS: [u16; 4] = [PROFILE_KIND, FOLLOWS_KIND, RELAY_LIST_KIND, BLOSSOM_AUTH_KIND];

/// How far `created_at` may be from the bunker's clock, either way. The editors set it a second
/// past the event they replace, so it can be slightly ahead.
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 600;
/// How long an upload authorisation may stay valid: the console asks for 5 minutes
/// (`frontend/app/utils/blossom.ts`), allowed a minute for the browser's clock.
pub const MAX_UPLOAD_AUTH_SECONDS: i64 = 300 + 60;
/// Size limits: generous for a follow list of thousands, far below anything abusive.
pub const MAX_CONTENT_BYTES: usize = 64 * 1024;
pub const MAX_TAGS: usize = 10_000;
pub const MAX_TAG_VALUES: usize = 8;
pub const MAX_TAG_VALUE_BYTES: usize = 1024;

/// What the console sends: an event template, never a pubkey, id or signature.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct EventTemplate {
    pub kind: u16,
    pub created_at: i64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// Why a template was refused: a short code and a message for the console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { code, message: message.into() }
}

/// The template as an event `identity` may sign, or why not. `now` is the bunker's clock, in
/// seconds.
pub fn check(template: &EventTemplate, identity: PublicKey, now: i64) -> Result<UnsignedEvent, Refusal> {
    if !CONSOLE_KINDS.contains(&template.kind) {
        return Err(refuse(
            "kind_not_allowed",
            format!("The console can only sign a profile, follow list, relay list or image upload, not kind {}.", template.kind),
        ));
    }
    if (template.created_at - now).abs() > MAX_CLOCK_SKEW_SECONDS {
        return Err(refuse("bad_created_at", "The event's time is too far from the bunker's clock. Check this computer's clock and try again."));
    }
    if template.content.len() > MAX_CONTENT_BYTES {
        return Err(refuse("too_large", "The event's content is too large."));
    }
    if template.tags.len() > MAX_TAGS {
        return Err(refuse("too_large", "The event has too many tags."));
    }
    for tag in &template.tags {
        if tag.is_empty() || tag.len() > MAX_TAG_VALUES || tag.iter().any(|value| value.len() > MAX_TAG_VALUE_BYTES) {
            return Err(refuse("bad_tags", "The event has a tag that is empty or too large."));
        }
    }
    match template.kind {
        PROFILE_KIND => check_profile(template)?,
        BLOSSOM_AUTH_KIND => check_upload_auth(template, now)?,
        _ => {}
    }

    let tags = template
        .tags
        .iter()
        .map(|tag| Tag::parse(tag.clone()).map_err(|_| refuse("bad_tags", "The event has a tag that could not be read.")))
        .collect::<Result<Vec<Tag>, Refusal>>()?;
    let created_at = u64::try_from(template.created_at).map_err(|_| refuse("bad_created_at", "The event's time is not valid."))?;
    Ok(UnsignedEvent::new(identity, Timestamp::from(created_at), Kind::from(template.kind), tags, template.content.clone()))
}

/// Kind 0: the content is a JSON object (NIP-01), as the profile editor writes it.
fn check_profile(template: &EventTemplate) -> Result<(), Refusal> {
    match serde_json::from_str::<serde_json::Value>(&template.content) {
        Ok(serde_json::Value::Object(_)) => Ok(()),
        _ => Err(refuse("bad_content", "A profile's content must be a JSON object.")),
    }
}

/// Kind 24242: a BUD-02 authorisation to upload one blob, as `blossom.ts` builds it. A `t` tag of
/// `upload` and no other verb, one `x` tag with a SHA-256, and an `expiration` in the future and no
/// more than a few minutes away, so a signed authorisation can't list, delete or be kept for later.
fn check_upload_auth(template: &EventTemplate, now: i64) -> Result<(), Refusal> {
    let verbs = tag_values(template, "t");
    if verbs != ["upload"] {
        return Err(refuse("bad_upload_auth", "An image upload authorisation must be for an upload only."));
    }
    let hashes = tag_values(template, "x");
    if hashes.len() != 1 || hashes[0].len() != 64 || !hashes[0].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(refuse("bad_upload_auth", "An image upload authorisation must name exactly one image by its SHA-256."));
    }
    let expirations = tag_values(template, "expiration");
    let expiration = match expirations.as_slice() {
        [one] => one.parse::<i64>().ok(),
        _ => None,
    };
    match expiration {
        Some(at) if at > now && at - now <= MAX_UPLOAD_AUTH_SECONDS => Ok(()),
        _ => Err(refuse("bad_upload_auth", "An image upload authorisation must expire within 5 minutes.")),
    }
}

/// The first value of every `name` tag, empty for a tag with none.
fn tag_values<'a>(template: &'a EventTemplate, name: &str) -> Vec<&'a str> {
    template.tags.iter().filter(|tag| tag[0] == name).map(|tag| tag.get(1).map(String::as_str).unwrap_or("")).collect()
}
