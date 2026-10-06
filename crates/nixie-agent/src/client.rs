//! Minimal HTTP client for the Nixie API.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, SystemTime};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Ping the API until it answers 200.
pub fn ping(address: &str) -> Result<(), String> {
    let mut backoff = Duration::from_secs(1);
    loop {
        match request(address, "GET", "/ping", None) {
            Ok((200, _)) => return Ok(()),
            Ok((status, _)) => eprintln!("unexpected response from Nixie API: {status}"),
            Err(error) => eprintln!("failed to ping Nixie API: {error}"),
        }

        let delay = backoff + jitter(backoff);
        eprintln!("retrying in {:?}", delay);
        std::thread::sleep(delay);

        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Request installation from the API.
pub fn install(address: &str, mac_address: &str) -> Result<(), String> {
    let body = format!("{{\"mac_address\":\"{mac_address}\"}}");
    match request(address, "POST", "/install", Some(&body))? {
        (202, response) => {
            println!("successfully requested installation: {}", response.trim());
            Ok(())
        }
        (status, _) => Err(format!("install request failed: {status}")),
    }
}

/// Perform one HTTP/1.1 request, returning the status code and response body.
fn request(
    address: &str,
    method: &str,
    path: &str,
    json_body: Option<&str>,
) -> Result<(u16, String), String> {
    let mut stream =
        TcpStream::connect(address).map_err(|error| format!("connecting to {address}: {error}"))?;
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;

    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    if let Some(body) = json_body {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    if let Some(body) = json_body {
        request.push_str(body);
    }

    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("sending request: {error}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("reading response: {error}"))?;

    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "malformed HTTP response".to_string())?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();

    Ok((status, body))
}

/// Jitter of up to half the backoff, to avoid a thundering herd.
fn jitter(backoff: Duration) -> Duration {
    let span = (backoff.as_nanos() / 2) as u64;
    if span == 0 {
        return Duration::ZERO;
    }
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    Duration::from_nanos(nanos % span)
}
