# proxydhcp-snooper

A small Rust prototype that tests how little code a Nixie ProxyDHCP/PXE snooper
needs. It mirrors the Nixie-specific parts of the Go implementation in
`internal/netboot/` (the `pixiecore` DHCP policy and the `dhcp4` raw socket),
delegating the DHCPv4 wire format to [`dhcproto`].

## Layout

- `src/proxy.rs` — pure firmware classification and ProxyDHCP offer
  construction. Mirrors `isBootDHCP`, `validateDHCP` and `offerDHCP` from the
  Go fork.
- `src/socket.rs` — Linux-only passive listener. Mirrors
  `internal/netboot/dhcp4/conn_linux.go`: an `AF_INET` / `SOCK_RAW` /
  `IPPROTO_UDP` socket with `IP_PKTINFO`, never binding UDP port 67, returning
  the ingress interface index with every packet.
- `src/main.rs` — the service loop.
- `tests/classification.rs` — decodes a real captured PXE DHCPDISCOVER and
  asserts the classification and offer fields. Needs no privileges.
- `tests/socket_e2e.rs` — privileged capture test, ignored by default.

## Toolchain

Use a matched Rust toolchain from nixpkgs:

    nix shell nixpkgs#cargo nixpkgs#rustc nixpkgs#clippy nixpkgs#rustfmt

## Build and test

    cargo build
    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

## End-to-end socket check

`tests/socket_e2e.rs` opens the real raw socket, injects the captured
DISCOVER, and checks that the offer comes back. It needs `CAP_NET_RAW` and
`CAP_NET_ADMIN`, so run the built test binary inside a user + network
namespace:

    cargo test --no-run
    unshare -Urn sh -c 'ip link set lo up; target/debug/deps/socket_e2e-* --ignored --nocapture'

The test creates a `pxetest0` dummy interface, injects the DISCOVER to
`10.9.9.1:67`, and asserts the captured interface index, the payload, and the
decoded offer.

## Deviations from the Go code

- Option 93/97/60/77 extraction and the offer fields are equivalent to the Go
  fork.
- The prototype takes the server IP on the command line instead of scanning the
  ingress interface's addresses (`interfaceIP` in Go). That lookup is a short
  `getifaddrs` helper and is not part of the risky socket/policy layer.
- `dhcproto`'s `Message::decode` keeps the NUL terminator when reading the
  fixed-size `sname`/`file` fields. This only affects round-tripped decoding in
  tests; the bytes written to the wire are unchanged.
