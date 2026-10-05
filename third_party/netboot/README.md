# netboot (vendored fork)

In-repo copy of [`go.universe.tf/netboot`](https://github.com/danderson/netboot),
taken at commit `2ed7bd30206a` (2024-05-31), Apache-2.0. See [LICENSE](./LICENSE).

The source files are kept **verbatim** from upstream; nothing under
`third_party/netboot` other than this file and `go.mod` has been edited. The
module is pulled in through a `replace` in the root `go.mod`:

```go
replace go.universe.tf/netboot => ./third_party/netboot
```

so all import paths stay `go.universe.tf/netboot/...` and the tree diffs cleanly
against the module cache.

`go.mod` here is ours, not upstream's. It declares `go 1.21` on purpose: that
keeps module graph pruning on (so we do not inherit upstream's `cobra`/`viper`
requirements for the CLI we do not use) while staying below `1.24`, which is
where `go vet`'s non-constant-format-string check would start rejecting this old
code. It also avoids the import rewrite and gofmt churn a plain in-tree copy
would need.

Only the packages Nixie uses are copied:

- `pixiecore` — PXE/iPXE boot server (`Server`, `Booter`, `Spec`, `Machine`).
- `dhcp4` — DHCPv4 server and packet codec.
- `dhcp6`, `dhcp6/pool` — DHCPv6 server and address pool (for PXE over IPv6).
- `tftp` — TFTP server. `pcap` is kept only as a test helper for `dhcp4`.

The upstream `out/ipxe` package (a 5 MiB generated `bindata.go` of iPXE firmware)
is not copied. The one asset we use, the x86_64-EFI firmware, is kept verbatim
as `internal/pxe/firmware/ipxe-x86_64.efi` and embedded with `go:embed`.

The upstream `pixiecore/cli` command, `cmd/`, `dockerfiles/`, and `scripts/`
are intentionally omitted: they pull in `cobra`/`viper` and are not part of the
code path Nixie uses.

This tree is expected to diverge from upstream as we remove unused code and
customize it; do not treat it as a tracking mirror.
