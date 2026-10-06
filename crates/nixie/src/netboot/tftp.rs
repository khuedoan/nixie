//! A read-only TFTP server.
//!
//! Ported from Nixie's `internal/netboot/tftp`: octet mode only, with the
//! `blksize` and `tsize` options, since those are what PXE firmware uses.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Duration a client has to acknowledge a data packet before it is resent.
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum number of times a packet is (re)sent before giving up.
pub const DEFAULT_WRITE_ATTEMPTS: u32 = 5;
/// Maximum block size offered to clients.
pub const DEFAULT_BLOCK_SIZE: u64 = 1450;

const MAX_ERROR_SIZE: usize = 500;

/// Provides the bytes for a requested path.
pub type Handler = Arc<dyn Fn(&str) -> anyhow::Result<(Vec<u8>, u64)> + Send + Sync>;

pub struct TftpServer {
    handler: Handler,
    write_timeout: Duration,
    write_attempts: u32,
    max_block_size: u64,
}

impl TftpServer {
    pub fn new(handler: Handler) -> Self {
        Self {
            handler,
            write_timeout: DEFAULT_WRITE_TIMEOUT,
            write_attempts: DEFAULT_WRITE_ATTEMPTS,
            max_block_size: DEFAULT_BLOCK_SIZE,
        }
    }

    /// Serve requests on `socket` until `shutdown` is set.
    pub fn serve(&self, socket: UdpSocket, shutdown: Arc<AtomicBool>) {
        let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
        let mut buf = [0u8; 512];
        while !shutdown.load(Ordering::Relaxed) {
            let (n, addr) = match socket.recv_from(&mut buf) {
                Ok(value) => value,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                Err(_) => continue,
            };

            let request = match parse_rrq(&buf[..n]) {
                Ok(request) => request,
                Err(error) => {
                    tracing::debug!(%addr, %error, "bad TFTP request");
                    continue;
                }
            };

            self.spawn(addr, request);
            tracing::debug!(%addr, "started TFTP transfer");
        }
    }

    fn spawn(&self, addr: SocketAddr, request: Rrq) {
        let handler = Arc::clone(&self.handler);
        let write_timeout = self.write_timeout;
        let write_attempts = self.write_attempts;
        let max_block_size = self.max_block_size;
        std::thread::spawn(move || {
            if let Err(error) = transfer(
                &handler,
                addr,
                request,
                write_timeout,
                write_attempts,
                max_block_size,
            ) {
                tracing::debug!(%addr, %error, "TFTP transfer failed");
            }
        });
    }
}

struct Rrq {
    filename: String,
    block_size: Option<u64>,
    want_size: bool,
}

fn transfer(
    handler: &Handler,
    addr: SocketAddr,
    request: Rrq,
    write_timeout: Duration,
    write_attempts: u32,
    max_block_size: u64,
) -> anyhow::Result<()> {
    let bind_addr = if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let conn = UdpSocket::bind(bind_addr)?;
    conn.connect(addr)?;

    let (data, size) = match handler(&request.filename) {
        Ok(value) => value,
        Err(error) => {
            let _ = conn.send(&error_packet(&format!("failed to get file: {error}")));
            return Err(error);
        }
    };

    let mut block_size = request.block_size.unwrap_or(0);
    if request.block_size.is_some() || (request.want_size && size != 0) {
        let mut payload = vec![0u8, 6];
        if request.block_size.is_some() {
            let clamped = block_size.min(max_block_size);
            block_size = clamped;
            payload.extend_from_slice(b"blksize\0");
            payload.extend_from_slice(clamped.to_string().as_bytes());
            payload.push(0);
        }
        if request.want_size && size != 0 {
            payload.extend_from_slice(b"tsize\0");
            payload.extend_from_slice(size.to_string().as_bytes());
            payload.push(0);
        }
        send_and_wait_ack(&conn, &payload, 0, write_timeout, write_attempts)?;
    }

    let block_size = if block_size == 0 { 512 } else { block_size } as usize;

    let mut seq: u16 = 1;
    let mut offset = 0usize;
    loop {
        let end = (offset + block_size).min(data.len());
        let chunk = &data[offset..end];
        let mut packet = Vec::with_capacity(4 + chunk.len());
        packet.extend_from_slice(&[0, 3]);
        packet.extend_from_slice(&seq.to_be_bytes());
        packet.extend_from_slice(chunk);
        send_and_wait_ack(&conn, &packet, seq, write_timeout, write_attempts)?;
        offset = end;
        seq = seq.wrapping_add(1);
        if chunk.len() < block_size {
            return Ok(());
        }
    }
}

fn send_and_wait_ack(
    conn: &UdpSocket,
    packet: &[u8],
    seq: u16,
    write_timeout: Duration,
    write_attempts: u32,
) -> anyhow::Result<()> {
    let mut recv = [0u8; 256];
    for _ in 0..write_attempts {
        conn.send(packet)?;
        conn.set_read_timeout(Some(write_timeout))?;

        loop {
            match conn.recv(&mut recv) {
                Ok(n) => {
                    if n < 4 {
                        continue;
                    }
                    match u16::from_be_bytes([recv[0], recv[1]]) {
                        4 if u16::from_be_bytes([recv[2], recv[3]]) == seq => return Ok(()),
                        5 => {
                            let (message, _) = tftp_str(&recv[4..]).unwrap_or_default();
                            anyhow::bail!("client aborted transfer: {message}");
                        }
                        _ => continue,
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break; // resend
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    anyhow::bail!("timeout waiting for ACK")
}

fn parse_rrq(bs: &[u8]) -> anyhow::Result<Rrq> {
    if bs.len() < 6 || u16::from_be_bytes([bs[0], bs[1]]) != 1 {
        anyhow::bail!("not an RRQ packet");
    }

    let (filename, mut rest) = tftp_str(&bs[2..])?;
    let (mode, tail) = tftp_str(rest)?;
    rest = tail;
    if mode != "octet" {
        anyhow::bail!("unsupported transfer mode {mode:?}");
    }

    let mut request = Rrq {
        filename,
        block_size: None,
        want_size: false,
    };

    while !rest.is_empty() {
        let (option, after_option) = tftp_str(rest)?;
        let (value, after_value) = tftp_str(after_option)?;
        rest = after_value;
        match option.as_str() {
            "tsize" => request.want_size = true,
            "blksize" => {
                let size: u64 = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("non-integer block size {value:?}"))?;
                if !(8..=65464).contains(&size) {
                    anyhow::bail!("unsupported block size {size}");
                }
                request.block_size = Some(size);
            }
            _ => {}
        }
    }

    Ok(request)
}

fn error_packet(message: &str) -> Vec<u8> {
    let truncated = if message.len() > MAX_ERROR_SIZE {
        &message[..MAX_ERROR_SIZE]
    } else {
        message
    };
    let mut packet = vec![0u8, 5, 0, 0];
    for byte in truncated.bytes() {
        match byte {
            b'\n' => packet.extend_from_slice(b"\r\n"),
            b'\r' => {}
            0x20..=0x7E => packet.push(byte),
            _ => packet.push(b'?'),
        }
    }
    packet.push(0);
    packet
}

/// Extract a NUL-terminated netascii string, returning the rest.
fn tftp_str(bs: &[u8]) -> anyhow::Result<(String, &[u8])> {
    for (i, byte) in bs.iter().enumerate() {
        if *byte == 0 {
            return Ok((String::from_utf8_lossy(&bs[..i]).into_owned(), &bs[i + 1..]));
        }
        if !(0x20..=0x7E).contains(byte) {
            anyhow::bail!("invalid netascii byte {byte:?} at offset {i}");
        }
    }
    anyhow::bail!("no null terminated string found")
}
