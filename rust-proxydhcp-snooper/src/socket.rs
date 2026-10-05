//! Passive raw-IP DHCP listener (Linux only).
//!
//! Mirrors `internal/netboot/dhcp4/conn_linux.go`: an `AF_INET` / `SOCK_RAW` /
//! `IPPROTO_UDP` socket with `IP_PKTINFO` enabled. It never binds UDP port 67,
//! so it coexists with a real DHCP server, and every received datagram carries
//! the interface index it arrived on.

use std::ffi::CString;
use std::io;
use std::mem;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

/// `sizeof(struct cmsghdr)` on 64-bit Linux.
const CMSG_HDR_LEN: usize = 16;
/// `sizeof(struct in_pktinfo)`.
const IP_PKTINFO_SIZE: usize = 12;

fn cmsg_align(len: usize) -> usize {
    let align = mem::size_of::<usize>();
    (len + align - 1) & !(align - 1)
}

/// One UDP datagram captured off the wire, with its ingress interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    pub payload: Vec<u8>,
    pub src: Ipv4Addr,
    pub src_port: u16,
    pub ifindex: u32,
}

/// A raw DHCP socket.
///
/// `local_port` is only used as the UDP source port when sending and to filter
/// received datagrams. The socket is never bound to it, which is the whole
/// point: it observes traffic addressed to port 67 without owning that port.
#[derive(Debug)]
pub struct RawDhcpSocket {
    fd: OwnedFd,
    local_port: u16,
}

impl RawDhcpSocket {
    /// Open the passive listener. Requires `CAP_NET_RAW`.
    pub fn open(local_port: u16) -> io::Result<Self> {
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_UDP) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };

        set_int_opt(fd.as_raw_fd(), libc::IPPROTO_IP, libc::IP_PKTINFO)?;
        set_int_opt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_BROADCAST)?;

        Ok(Self { fd, local_port })
    }

    /// Set or clear the receive timeout.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        let tv = match timeout {
            None => libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            Some(d) => libc::timeval {
                tv_sec: d.as_secs() as libc::time_t,
                tv_usec: d.subsec_micros() as libc::suseconds_t,
            },
        };
        let rc = unsafe {
            libc::setsockopt(
                self.fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&tv as *const libc::timeval).cast(),
                mem::size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Block until a UDP datagram addressed to `local_port` is captured.
    ///
    /// Datagrams to other ports are skipped. On timeout the underlying
    /// `recvmsg` returns `WouldBlock`/`TimedOut`.
    pub fn recv(&self) -> io::Result<Datagram> {
        loop {
            if let Some(dg) = self.recv_one()? {
                return Ok(dg);
            }
        }
    }

    fn recv_one(&self) -> io::Result<Option<Datagram>> {
        let mut buf = vec![0u8; 65535];
        let mut ctrl = [0u64; 64];
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        let mut msg: libc::msghdr = unsafe { mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = ctrl.as_mut_ptr().cast();
        msg.msg_controllen = mem::size_of_val(&ctrl);

        let n = unsafe { libc::recvmsg(self.fd.as_raw_fd(), &mut msg, 0) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        let n = n as usize;

        let ifindex = unsafe { pktinfo_ifindex(msg.msg_control, msg.msg_controllen) };

        // Raw IPv4 sockets include the IP header.
        if n < 20 {
            return Ok(None);
        }
        let ihl = ((buf[0] & 0x0f) as usize) * 4;
        if ihl < 20 || n < ihl + 8 {
            return Ok(None);
        }
        let src = Ipv4Addr::new(buf[12], buf[13], buf[14], buf[15]);
        let udp = &buf[ihl..n];
        let src_port = u16::from_be_bytes([udp[0], udp[1]]);
        let dst_port = u16::from_be_bytes([udp[2], udp[3]]);
        if dst_port != self.local_port {
            return Ok(None);
        }
        Ok(Some(Datagram {
            payload: udp[8..].to_vec(),
            src,
            src_port,
            ifindex,
        }))
    }

    /// Send `payload` as a UDP datagram from `local_port` to `dst:dst_port`,
    /// forced out `ifindex`. The checksum is left zero, matching Go.
    pub fn send(
        &self,
        payload: &[u8],
        dst: Ipv4Addr,
        dst_port: u16,
        ifindex: u32,
    ) -> io::Result<()> {
        let mut udp = Vec::with_capacity(8 + payload.len());
        udp.extend_from_slice(&self.local_port.to_be_bytes());
        udp.extend_from_slice(&dst_port.to_be_bytes());
        udp.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
        udp.extend_from_slice(&[0, 0]);
        udp.extend_from_slice(payload);

        let mut addr: libc::sockaddr_in = unsafe { mem::zeroed() };
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = dst_port.to_be();
        addr.sin_addr.s_addr = u32::from_ne_bytes(dst.octets());

        let mut ctrl = [0u64; 8];
        let ctrl_len = if ifindex != 0 {
            unsafe { write_pktinfo(ctrl.as_mut_ptr().cast(), ifindex) };
            cmsg_align(CMSG_HDR_LEN + IP_PKTINFO_SIZE)
        } else {
            0
        };

        let mut iov = libc::iovec {
            iov_base: udp.as_ptr() as *mut libc::c_void,
            iov_len: udp.len(),
        };
        let mut msg: libc::msghdr = unsafe { mem::zeroed() };
        msg.msg_name = (&mut addr as *mut libc::sockaddr_in).cast();
        msg.msg_namelen = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = ctrl.as_mut_ptr().cast();
        msg.msg_controllen = ctrl_len;

        let n = unsafe { libc::sendmsg(self.fd.as_raw_fd(), &msg, 0) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

fn set_int_opt(fd: libc::c_int, level: libc::c_int, name: libc::c_int) -> io::Result<()> {
    let on: libc::c_int = 1;
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            (&on as *const libc::c_int).cast(),
            mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Resolve an interface name to its index.
pub fn interface_index(name: &str) -> io::Result<u32> {
    let cname = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let index = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    if index == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(index)
}

/// Walk the control messages and return `ipi_ifindex`, or 0 if absent.
unsafe fn pktinfo_ifindex(control: *mut libc::c_void, controllen: usize) -> u32 {
    let mut off = 0usize;
    let base = control as *const u8;
    while off + CMSG_HDR_LEN + IP_PKTINFO_SIZE <= controllen {
        let hdr = base.add(off);
        let len = (hdr as *const usize).read_unaligned();
        let level = (hdr.add(8) as *const libc::c_int).read_unaligned();
        let kind = (hdr.add(12) as *const libc::c_int).read_unaligned();
        if len < CMSG_HDR_LEN || off + len > controllen {
            break;
        }
        if level == libc::IPPROTO_IP && kind == libc::IP_PKTINFO {
            let ifindex = (hdr.add(CMSG_HDR_LEN) as *const libc::c_int).read_unaligned();
            return ifindex as u32;
        }
        off += cmsg_align(len);
    }
    0
}

/// Write a single `IP_PKTINFO` control message carrying `ifindex`.
unsafe fn write_pktinfo(buf: *mut u8, ifindex: u32) {
    let len = CMSG_HDR_LEN + IP_PKTINFO_SIZE;
    (buf as *mut usize).write_unaligned(len);
    (buf.add(8) as *mut libc::c_int).write_unaligned(libc::IPPROTO_IP);
    (buf.add(12) as *mut libc::c_int).write_unaligned(libc::IP_PKTINFO);
    let data = buf.add(CMSG_HDR_LEN);
    (data as *mut libc::c_int).write_unaligned(ifindex as libc::c_int); // ipi_ifindex
    (data.add(4) as *mut u32).write_unaligned(0); // ipi_spec_dst
    (data.add(8) as *mut u32).write_unaligned(0); // ipi_addr
}
