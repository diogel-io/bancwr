//! Requests held for an administrator's approval before signing (#102).
//!
//! A token may mark some of its kinds as needing approval. A `sign_event` for one of them is not
//! signed: `Nip46Handler` parks it here and answers the app at once with a NIP-46 auth challenge
//! (`auth_url`) pointing at the console, when the console's address is known. The real answer is
//! sent later, under the same request id, when an administrator approves or rejects it, when it
//! times out, or when its connection ends.
//!
//! The queue is in memory only, so event content is never written to disk or into a backup. A
//! restart drops every request still waiting: the app gets no answer, and the audit log keeps the
//! `approval_requested` with no outcome. It is bounded, per connection and in total.
use crate::db::Nip46Connection;
use chrono::{DateTime, Duration, Utc};
use nostr::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// At most this many requests wait for one connection, and for the whole bunker.
pub const MAX_PENDING_PER_CONNECTION: usize = 20;
pub const MAX_PENDING: usize = 200;

/// The longest note an administrator may give with a decision.
pub const MAX_NOTE_CHARS: usize = 280;

/// How a request reached the bunker, so its later answer is encrypted the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Nip44,
    Nip04,
}

/// A `sign_event` waiting for a decision.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: Uuid,
    /// The NIP-46 request id, which the eventual answer reuses.
    pub request_id: String,
    pub client_pubkey: PublicKey,
    pub scheme: Scheme,
    /// The connection as it was when the request arrived.
    pub connection: Nip46Connection,
    /// The event as it will be signed: the bunker's key, the app's `created_at`, kind, tags and
    /// content, and its id computed.
    pub event: UnsignedEvent,
    pub event_id: EventId,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl Pending {
    pub fn kind(&self) -> u16 {
        self.event.kind.as_u16()
    }

    /// The fixed part of every audit event about this request.
    pub fn audit_detail(&self) -> String {
        format!("request={};kind={};event_id={}", self.id, self.kind(), self.event_id.to_hex())
    }
}

/// Why `hold` refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueFull {
    Connection,
    Bunker,
}

/// The requests waiting, by id.
#[derive(Debug, Default)]
pub struct ApprovalQueue {
    pending: Mutex<HashMap<Uuid, Pending>>,
}

impl ApprovalQueue {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Pending>> {
        self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Parks `pending`, unless its connection or the bunker already has as many waiting as allowed.
    pub fn hold(&self, pending: Pending) -> Result<(), QueueFull> {
        let mut queue = self.lock();
        if queue.len() >= MAX_PENDING {
            return Err(QueueFull::Bunker);
        }
        if queue.values().filter(|p| p.connection.id == pending.connection.id).count() >= MAX_PENDING_PER_CONNECTION {
            return Err(QueueFull::Connection);
        }
        queue.insert(pending.id, pending);
        Ok(())
    }

    /// A copy of one waiting request.
    pub fn get(&self, id: Uuid) -> Option<Pending> {
        self.lock().get(&id).cloned()
    }

    /// Removes and returns one waiting request: whoever takes it decides it, so it is decided once.
    pub fn take(&self, id: Uuid) -> Option<Pending> {
        self.lock().remove(&id)
    }

    /// Every request waiting, oldest first, or only those for `for_pubkey`.
    pub fn list(&self, for_pubkey: Option<&str>) -> Vec<Pending> {
        let mut pending: Vec<Pending> = self.lock().values().filter(|p| for_pubkey.is_none_or(|key| p.connection.for_pubkey == key)).cloned().collect();
        pending.sort_by(|a, b| a.requested_at.cmp(&b.requested_at).then(a.id.cmp(&b.id)));
        pending
    }

    /// Removes and returns every request that has reached its expiry at `now`.
    pub fn take_expired(&self, now: DateTime<Utc>) -> Vec<Pending> {
        self.take_where(|p| p.expires_at <= now)
    }

    /// Removes and returns every request for which `ended` is true.
    pub fn take_where(&self, ended: impl Fn(&Pending) -> bool) -> Vec<Pending> {
        let mut queue = self.lock();
        let ids: Vec<Uuid> = queue.values().filter(|p| ended(p)).map(|p| p.id).collect();
        let mut taken: Vec<Pending> = ids.iter().filter_map(|id| queue.remove(id)).collect();
        taken.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
        taken
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A decision's note: trimmed, on one line, at most `MAX_NOTE_CHARS`. None when empty.
pub fn clean_note(note: Option<&str>) -> Option<String> {
    let note: String = note?.split_whitespace().collect::<Vec<_>>().join(" ");
    let note: String = note.chars().filter(|c| !c.is_control()).take(MAX_NOTE_CHARS).collect();
    (!note.is_empty()).then_some(note)
}

/// "30 minutes", "1 minute", "45 seconds": how long a request waited before it timed out.
pub fn describe_timeout(timeout: Duration) -> String {
    let seconds = timeout.num_seconds().max(0);
    let (value, unit) = if seconds >= 60 && seconds % 60 == 0 { (seconds / 60, "minute") } else { (seconds, "second") };
    format!("{} {}{}", value, unit, if value == 1 { "" } else { "s" })
}

/// What the app is told when an administrator rejects its request.
pub fn rejected_message(note: Option<&str>) -> String {
    match note {
        Some(note) => format!("Rejected by an administrator: {}", note),
        None => "Rejected by an administrator".to_string(),
    }
}

/// What the app is told when nobody decided in time.
pub fn timed_out_message(timeout: Duration) -> String {
    format!("Approval timed out after {}: nothing was signed", describe_timeout(timeout))
}

/// What the app is told when its connection ended while the request waited.
pub const CONNECTION_ENDED: &str = "This connection ended: nothing was signed";

/// What the app is told when too many of its, or the bunker's, requests are already waiting.
pub const QUEUE_FULL: &str = "Too many requests are waiting for approval";
