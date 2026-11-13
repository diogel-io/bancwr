//! `bunker healthcheck`: the container's health probe (#16). The runtime image is distroless, with
//! no shell and no curl, so the binary checks itself: `GET /health` on the local port, over a plain
//! TCP connection, exiting 0 when the bunker answers 200 and 1 otherwise.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// How long the probe waits to connect, and then for an answer.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// `Ok` when `GET /health` on 127.0.0.1:`port` answers 200.
pub fn check(port: u16) -> Result<(), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT).map_err(|e| format!("cannot connect to {}: {}", address, e))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .map_err(|e| format!("cannot send the request: {}", e))?;

    // Read the whole answer (the request asked to close), up to a limit: the status line decides,
    // and a locked bunker's body says why (#94).
    let mut response = Vec::new();
    let mut buffer = [0u8; 1024];
    while response.len() < MAX_RESPONSE {
        let read = match stream.read(&mut buffer) {
            Ok(read) => read,
            Err(e) if response.is_empty() => return Err(format!("no answer: {}", e)),
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
    }
    let response = String::from_utf8_lossy(&response);
    let status_line = response.lines().next().unwrap_or_default();
    match status_line.split_whitespace().nth(1) {
        Some("200") => Ok(()),
        _ => match locked_reason(&response) {
            Some(reason) => Err(format!("unhealthy: {:?}: locked: {}", status_line, reason)),
            None => Err(format!("unhealthy: {:?}", status_line)),
        },
    }
}

/// The most of an answer the probe reads.
const MAX_RESPONSE: usize = 16 * 1024;

/// The `reason` in a locked bunker's `/health` body (#94), if the answer carries one.
pub fn locked_reason(response: &str) -> Option<String> {
    let (_, body) = response.split_once("\r\n\r\n")?;
    let body: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    body.get("reason")?.as_str().map(str::to_string)
}

/// The port the bunker listens on, as `Config` reads it, without loading the rest of the config:
/// the probe must not need the signing key.
pub fn port_from_env() -> u16 {
    std::env::var("BUNKER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000)
}
