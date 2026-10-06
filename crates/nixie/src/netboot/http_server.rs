//! A tiny HTTP/1.1 server for serving boot files.
//!
//! `tiny_http` does not set `TCP_NODELAY`, which makes bulk transfers to
//! firmware TCP stacks crawl (Nagle plus delayed ACKs). This server sets it and
//! streams files in large chunks, matching what Go's `net/http` did.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(120);
const COPY_BUFFER: usize = 256 * 1024;

/// A response body, either in memory or streamed from a file.
pub enum Body {
    Bytes(Vec<u8>),
    File { file: File, size: u64 },
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Body,
}

impl Response {
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain",
            body: Body::Bytes(body.into().into_bytes()),
        }
    }

    pub fn bytes(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body: Body::Bytes(body),
        }
    }

    pub fn file(file: File, size: u64) -> Self {
        Self {
            status: 200,
            content_type: "application/octet-stream",
            body: Body::File { file, size },
        }
    }
}

/// Handles a request: method, target (path plus query) and Host header.
pub type Handler = Arc<dyn Fn(&str, &str, &str) -> Response + Send + Sync>;

/// Serve requests until `shutdown` is set.
pub fn serve(
    listener: TcpListener,
    handler: Handler,
    shutdown: Arc<AtomicBool>,
) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let handler = Arc::clone(&handler);
                std::thread::spawn(move || {
                    if let Err(error) = handle(stream, handler) {
                        tracing::debug!(%error, "HTTP connection failed");
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, handler: Handler) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    let mut host = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Host:") {
            host = value.trim().to_string();
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    let response = handler(&method, &target, &host);
    write_response(&mut stream, response)
}

fn write_response(stream: &mut TcpStream, response: Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let length = match &response.body {
        Body::Bytes(bytes) => bytes.len() as u64,
        Body::File { size, .. } => *size,
    };

    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
        response.status, reason, length, response.content_type
    )?;

    match response.body {
        Body::Bytes(bytes) => stream.write_all(&bytes)?,
        Body::File { mut file, .. } => {
            let mut buffer = vec![0u8; COPY_BUFFER];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                stream.write_all(&buffer[..read])?;
            }
        }
    }

    stream.flush()
}
