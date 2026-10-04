//! Container healthcheck: GET /healthz on localhost with std only.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub fn run(port: u16) -> ! {
    match check(port) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("healthcheck failed: {e}");
            std::process::exit(1);
        }
    }
}

fn check(port: u16) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let timeout = Duration::from_secs(2);
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let head = String::from_utf8_lossy(&buf);
    let status = head.lines().next().unwrap_or("");
    if status.starts_with("HTTP/1.1 200") || status.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        Err(format!("unexpected response: {status:?}"))
    }
}
