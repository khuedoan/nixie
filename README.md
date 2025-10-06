# Nixie

NixOS PXE boot install with
[Wake-on-LAN](https://en.wikipedia.org/wiki/Wake-on-LAN),
[Pixiecore](https://github.com/danderson/netboot/tree/main/pixiecore) and
[nixos-anywhere](https://nix-community.github.io/nixos-anywhere).

## Usage

```sh
nixie \
    --flake ./examples \
    --installer ./examples#installer \
    --hosts ./examples/hosts.json
```
