//! Per-connection rate limits for NIP-46 (#101).
//!
//! Each connection has a token bucket: it holds up to `burst` requests and refills at
//! `per_minute / 60` a second. A request that finds the bucket empty is refused. Only the methods
//! that use the bunker's key for the app count (`COUNTED_METHODS`); the check runs in
//! `Nip46Handler::handle_request`, because NIP-46 traffic arrives over relays, where no reverse
//! proxy can limit it.
//!
//! The buckets are kept in memory: a restart refills them all. That is acceptable, because the
//! limit bounds a runaway or compromised app over minutes, not across restarts.
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How fast one connection may use the bunker's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    /// The steady rate the bucket refills at.
    pub per_minute: u32,
    /// How many requests the bucket holds, so how many may come at once.
    pub burst: u32,
}

impl RateLimit {
    /// The limit of a token issued without one, and of every connection made before #101.
    pub const DEFAULT: RateLimit = RateLimit { per_minute: 60, burst: 20 };
    /// The rates an administrator may choose.
    pub const PER_MINUTE: RangeInclusive<u32> = 1..=600;
    /// The bursts an administrator may choose.
    pub const BURST: RangeInclusive<u32> = 1..=100;

    /// A limit from optional parts, each defaulting on its own. None when both are absent, which is
    /// stored as NULL and means the default. Err when either part is out of range.
    pub fn from_parts(per_minute: Option<u32>, burst: Option<u32>) -> Result<Option<RateLimit>, String> {
        if per_minute.is_none() && burst.is_none() {
            return Ok(None);
        }
        let limit = RateLimit { per_minute: per_minute.unwrap_or(Self::DEFAULT.per_minute), burst: burst.unwrap_or(Self::DEFAULT.burst) };
        if !Self::PER_MINUTE.contains(&limit.per_minute) {
            return Err(format!("rate_per_minute must be from {} to {}", Self::PER_MINUTE.start(), Self::PER_MINUTE.end()));
        }
        if !Self::BURST.contains(&limit.burst) {
            return Err(format!("rate_burst must be from {} to {}", Self::BURST.start(), Self::BURST.end()));
        }
        Ok(Some(limit))
    }

    /// The limit in force for a stored one: NULL is the default.
    pub fn effective(stored: Option<RateLimit>) -> RateLimit {
        stored.unwrap_or(Self::DEFAULT)
    }

    fn per_second(&self) -> f64 {
        f64::from(self.per_minute) / 60.0
    }
}

/// The NIP-46 methods that count against the limit: those that use the bunker's key for the app.
/// `connect`, `ping`, `get_public_key`, `switch_relays` and `logout` do not.
pub const COUNTED_METHODS: [&str; 5] = ["sign_event", "nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"];

/// Whether `method` counts against the limit.
pub fn counts(method: &str) -> bool {
    COUNTED_METHODS.contains(&method)
}

/// How often a connection that keeps being refused is reported (logged and audited).
pub const REPORT_INTERVAL: Duration = Duration::from_secs(60);

/// How far below a whole request a bucket may be and still count as holding it.
const TOLERANCE: f64 = 1e-9;

/// What `RateLimiter::check` decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Limited {
        /// Whole seconds until the bucket holds a request again, at least 1.
        retry_after_secs: u64,
        /// True at most once per `REPORT_INTERVAL` per connection: log and audit this refusal.
        report: bool,
    },
}

#[derive(Debug)]
struct Bucket {
    limit: RateLimit,
    tokens: f64,
    updated: Instant,
    reported: Option<Instant>,
}

impl Bucket {
    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.updated).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.limit.per_second()).min(f64::from(self.limit.burst));
        self.updated = now;
    }

    fn reported_within(&self, now: Instant) -> bool {
        self.reported.is_some_and(|at| now.saturating_duration_since(at) < REPORT_INTERVAL)
    }
}

/// The buckets of every connection that has used the key recently, keyed by connection id.
#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one request from the connection's bucket, now.
    pub fn check(&self, connection_id: &str, limit: RateLimit) -> Decision {
        self.check_at(connection_id, limit, Instant::now())
    }

    /// `check` as at `now`, for tests.
    pub fn check_at(&self, connection_id: &str, limit: RateLimit, now: Instant) -> Decision {
        let mut buckets = self.buckets.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let bucket = buckets
            .entry(connection_id.to_string())
            .or_insert_with(|| Bucket { limit, tokens: f64::from(limit.burst), updated: now, reported: None });
        if bucket.limit != limit {
            // A connection's limit is fixed when it connects, so this is only a defensive reset.
            *bucket = Bucket { limit, tokens: f64::from(limit.burst), updated: now, reported: None };
        }
        bucket.refill(now);
        // A refill added up in parts can fall a rounding error short of a whole request.
        if bucket.tokens >= 1.0 - TOLERANCE {
            bucket.tokens -= 1.0;
            return Decision::Allowed;
        }
        let retry_after_secs = ((((1.0 - bucket.tokens) / limit.per_second()) - TOLERANCE).ceil() as u64).max(1);
        let report = !bucket.reported_within(now);
        if report {
            bucket.reported = Some(now);
        }
        Decision::Limited { retry_after_secs, report }
    }

    /// Drops every bucket that has refilled and has not been reported within `REPORT_INTERVAL`,
    /// which includes those of connections that ended. Such a bucket is the same as none, so this
    /// only bounds memory. Called hourly (`main.rs`).
    pub fn sweep(&self) -> usize {
        self.sweep_at(Instant::now())
    }

    /// `sweep` as at `now`, for tests. Returns how many buckets it dropped.
    pub fn sweep_at(&self, now: Instant) -> usize {
        let mut buckets = self.buckets.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = buckets.len();
        buckets.retain(|_, bucket| {
            bucket.refill(now);
            bucket.tokens < f64::from(bucket.limit.burst) - TOLERANCE || bucket.reported_within(now)
        });
        before - buckets.len()
    }

    /// How many buckets are held.
    pub fn len(&self) -> usize {
        self.buckets.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
