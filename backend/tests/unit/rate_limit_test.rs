// The per-connection token bucket (#101): the burst, refill, the retry time, reporting at most once
// a minute, independent connections, and the sweep. Time is passed in, so nothing here sleeps.
use bunker::rate_limit::{counts, Decision, RateLimit, RateLimiter, REPORT_INTERVAL};
use std::time::{Duration, Instant};

const TEN_A_MINUTE: RateLimit = RateLimit { per_minute: 10, burst: 3 };

fn limited(decision: Decision) -> (u64, bool) {
    match decision {
        Decision::Limited { retry_after_secs, report } => (retry_after_secs, report),
        Decision::Allowed => panic!("expected a refusal"),
    }
}

#[test]
fn allows_the_burst_then_refuses_the_next() {
    let limiter = RateLimiter::new();
    let start = Instant::now();
    for _ in 0..3 {
        assert_eq!(limiter.check_at("c1", TEN_A_MINUTE, start), Decision::Allowed);
    }
    let (retry, report) = limited(limiter.check_at("c1", TEN_A_MINUTE, start));
    // 10 a minute is one every 6 s.
    assert_eq!(retry, 6);
    assert!(report);
}

#[test]
fn refills_at_the_rate_and_never_above_the_burst() {
    let limiter = RateLimiter::new();
    let start = Instant::now();
    for _ in 0..3 {
        limiter.check_at("c1", TEN_A_MINUTE, start);
    }
    // 5 s is not yet one request; 6 s is.
    assert!(matches!(limiter.check_at("c1", TEN_A_MINUTE, start + Duration::from_secs(5)), Decision::Limited { .. }));
    assert_eq!(limiter.check_at("c1", TEN_A_MINUTE, start + Duration::from_secs(6)), Decision::Allowed);

    // An hour idle refills only to the burst.
    let later = start + Duration::from_secs(3600);
    for _ in 0..3 {
        assert_eq!(limiter.check_at("c1", TEN_A_MINUTE, later), Decision::Allowed);
    }
    assert!(matches!(limiter.check_at("c1", TEN_A_MINUTE, later), Decision::Limited { .. }));
}

#[test]
fn a_fractional_rate_refills_in_parts_and_the_retry_rounds_up() {
    let limiter = RateLimiter::new();
    let limit = RateLimit { per_minute: 7, burst: 1 };
    let start = Instant::now();
    assert_eq!(limiter.check_at("c1", limit, start), Decision::Allowed);
    // 7 a minute is one every 60/7 = 8.57 s, so 9 s to wait.
    assert_eq!(limited(limiter.check_at("c1", limit, start)).0, 9);
    assert_eq!(limited(limiter.check_at("c1", limit, start + Duration::from_secs(4))).0, 5);
    assert_eq!(limiter.check_at("c1", limit, start + Duration::from_millis(8600)), Decision::Allowed);
}

#[test]
fn the_retry_is_at_least_a_second() {
    let limiter = RateLimiter::new();
    let limit = RateLimit { per_minute: 600, burst: 1 };
    let start = Instant::now();
    limiter.check_at("c1", limit, start);
    assert_eq!(limited(limiter.check_at("c1", limit, start + Duration::from_millis(50))).0, 1);
}

#[test]
fn reports_at_most_once_a_minute_while_refusals_continue() {
    let limiter = RateLimiter::new();
    let limit = RateLimit { per_minute: 1, burst: 1 };
    let start = Instant::now();
    limiter.check_at("c1", limit, start);
    let reports: Vec<bool> = (0..5).map(|i| limited(limiter.check_at("c1", limit, start + Duration::from_secs(i * 10))).1).collect();
    assert_eq!(reports, vec![true, false, false, false, false]);
    // At 60 s a request is allowed again; refused again just after, and reported, because a minute
    // has passed since the last report.
    assert_eq!(limiter.check_at("c1", limit, start + REPORT_INTERVAL), Decision::Allowed);
    assert!(limited(limiter.check_at("c1", limit, start + REPORT_INTERVAL)).1);
}

#[test]
fn each_connection_has_its_own_bucket() {
    let limiter = RateLimiter::new();
    let start = Instant::now();
    for _ in 0..3 {
        limiter.check_at("c1", TEN_A_MINUTE, start);
    }
    assert!(matches!(limiter.check_at("c1", TEN_A_MINUTE, start), Decision::Limited { .. }));
    assert_eq!(limiter.check_at("c2", TEN_A_MINUTE, start), Decision::Allowed);
    assert_eq!(limiter.len(), 2);
}

#[test]
fn the_sweep_drops_only_full_buckets_not_reported_within_a_minute() {
    let limiter = RateLimiter::new();
    let limit = RateLimit { per_minute: 60, burst: 2 };
    let start = Instant::now();
    limiter.check_at("idle", limit, start);
    for _ in 0..3 {
        limiter.check_at("refused", limit, start);
    }
    // At 0.5 s neither has refilled.
    assert_eq!(limiter.sweep_at(start + Duration::from_millis(500)), 0);
    // At 30 s both have, but "refused" was reported within the minute.
    assert_eq!(limiter.sweep_at(start + Duration::from_secs(30)), 1);
    assert_eq!(limiter.len(), 1);
    assert_eq!(limiter.sweep_at(start + REPORT_INTERVAL), 1);
    assert!(limiter.is_empty());
    // A swept connection starts again with a full bucket.
    assert_eq!(limiter.check_at("idle", limit, start + REPORT_INTERVAL), Decision::Allowed);
}

#[test]
fn only_the_methods_that_use_the_key_count() {
    for method in ["sign_event", "nip44_encrypt", "nip44_decrypt", "nip04_encrypt", "nip04_decrypt"] {
        assert!(counts(method), "{}", method);
    }
    for method in ["connect", "ping", "get_public_key", "switch_relays", "logout", "unknown"] {
        assert!(!counts(method), "{}", method);
    }
}

#[test]
fn limits_default_each_part_on_its_own_and_refuse_out_of_range() {
    assert_eq!(RateLimit::DEFAULT, RateLimit { per_minute: 60, burst: 20 });
    assert_eq!(RateLimit::from_parts(None, None), Ok(None));
    assert_eq!(RateLimit::from_parts(Some(120), None), Ok(Some(RateLimit { per_minute: 120, burst: 20 })));
    assert_eq!(RateLimit::from_parts(None, Some(5)), Ok(Some(RateLimit { per_minute: 60, burst: 5 })));
    assert_eq!(RateLimit::from_parts(Some(1), Some(1)), Ok(Some(RateLimit { per_minute: 1, burst: 1 })));
    assert_eq!(RateLimit::from_parts(Some(600), Some(100)), Ok(Some(RateLimit { per_minute: 600, burst: 100 })));
    for (per_minute, burst) in [(Some(0), None), (Some(601), None), (None, Some(0)), (None, Some(101)), (Some(10), Some(1000))] {
        assert!(RateLimit::from_parts(per_minute, burst).is_err(), "{:?} {:?}", per_minute, burst);
    }
    assert_eq!(RateLimit::effective(None), RateLimit::DEFAULT);
}
