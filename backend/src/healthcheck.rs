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

    // The status line is all that is needed, and it arrives first.
    let mut response = Vec::new();
    let mut buffer = [0u8; 256];
    while !response.contains(&b'\n') {
        let read = stream.read(&mut buffer).map_err(|e| format!("no answer: {}", e))?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
    }
    let status_line = String::from_utf8_lossy(&response);
    let status_line = status_line.lines().next().unwrap_or_default();
    match status_line.split_whitespace().nth(1) {
        Some("200") => Ok(()),
        _ => Err(format!("unhealthy: {:?}", status_line)),
    }
}

/// The port the bunker listens on, as `Config` reads it, without loading the rest of the config:
/// the probe must not need the signing key.
pub fn port_from_env() -> u16 {
    std::env::var("BUNKER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000)
}
