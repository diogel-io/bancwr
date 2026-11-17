// Ports for the tests that start a mock relay or a `bunker serve` child process (#114). Both pick
// a port that was free when probed and bind it later, so under the full suite another socket can
// take it in between and the start fails with `AddrInUse`. These helpers retry that start.
//
// No `bunker` dependencies, so unit and integration tests can both include it:
//   #[path = "../support/ports.rs"]
//   mod ports;
#![allow(dead_code)]

use nostr_relay_builder::{Error, MockRelay};
use std::io::ErrorKind;
use std::time::Duration;

/// How many times a start is tried before the test fails.
pub const ATTEMPTS: usize = 5;

/// A port that was free a moment ago. Another socket can take it before it is bound, so whatever
/// binds it must retry on `AddrInUse`.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// `MockRelay::run()`, retried on `AddrInUse`. nostr-relay-builder 0.39 picks a random port,
/// probes it and binds it afterwards (`local/util.rs`, `local/inner.rs`), and has no way to take a
/// listener that is already bound. `RelayBuilder::port(0)` would bind, but `url()` would then
/// report port 0.
pub async fn run_mock_relay() -> MockRelay {
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        match MockRelay::run().await {
            Ok(relay) => return relay,
            Err(Error::IO(e)) if e.kind() == ErrorKind::AddrInUse => {
                last = Some(e);
                if attempt < ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            Err(e) => panic!("the mock relay did not start: {}", e),
        }
    }
    panic!("the mock relay did not start after {} attempts: {}", ATTEMPTS, last.unwrap());
}
