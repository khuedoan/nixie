# Nixie

NixOS PXE boot install with
[Wake-on-LAN](https://en.wikipedia.org/wiki/Wake-on-LAN),
[Pixiecore](https://github.com/danderson/netboot/tree/main/pixiecore) and
[nixos-anywhere](https://nix-community.github.io/nixos-anywhere).
Currently, only `x86_64-linux` is supported.

## Features

- [x] Simple, declarative JSON configuration
- [x] Build a custom NixOS installer from a flake
- [x] Built-in PXE server to serve netboot components from the custom installer
- [ ] Host status check with IP discovery
- [ ] Remote power-on with Wake-on-LAN
- [ ] Custom agent and API to manage the installation process
- [ ] Install NixOS from a flake using nixos-anywhere
- [x] Stateless

## Usage

Example command to boot a custom NixOS installer and install the corresponding
NixOS configuration from [`./examples/flake.nix`](./examples/flake.nix) on
multiple bare-metal machines based on the MAC addresses defined in
[`./examples/hosts.json`](./examples/hosts.json).

```sh
# Running as root for privileged ports
sudo nixie \
    --installer ./examples#nixosConfigurations.installer \
    --flake ./examples \
    --hosts ./examples/hosts.json \
    --ssh-key ~/.ssh/id_ed25519
```

TODO add a demo video/asciinema.

Please see the full example in [`./examples`](./examples).

## How it works

TODO refine the diagram after implementation.

```mermaid
sequenceDiagram
    participant Nix
    participant Nixie
    participant Machines@{ "type" : "collections" }

    Nixie->>Nixie: Load hosts.json

    loop For each machine
    Nixie->>Machines: Try checking status
    Nixie->>Nixie: Skip if already installed
    end

    Nixie->>Nix: Build installer components<br/>(kernel, initrd, squashfs)
    Nixie->>Nixie: Start server components in goroutines<br/>(DHCP/TFTP/HTTP/API)

    loop For each machine
        Nixie->>Machines: Broadcast Wake-on-LAN magic packet

        activate Machines

        Note over Machines: Power on and start PXE boot

        Machines->>Nixie: UEFI firmware broadcast DHCP request
        Nixie->>Machines: DHCP provide IP (via Proxy DHCP) and next server info
        Machines->>Nixie: Request kernel
        Nixie->>Machines: TFTP send kernel
        Machines->>Nixie: Request initrd
        Nixie->>Machines: TFTP send initrd

        Note over Machines: Boot into NixOS installer
        Note over Machines: SystemD starts nixie-agent service
        Machines->>Nixie: nixie-agent phone home to request install with MAC address
        Nixie->>Nixie: Find flake based on MAC address and get client IP from API request
        Nixie->>Nix: Build NixOS configuration
        Nixie->>Machines: nixos-anywhere format disks via SSH based on disko configuration
        Nixie->>Machines: nixos-anywhere install system closure via SSH
        Nixie->>Machines: nixos-anywhere trigger reboot

        Note over Machines: Reboot after installation completed

        Nixie->>Machines: nixos-anywhere confirms machine rebooted
        deactivate Machines

        activate Machines

        Nixie->>Machines: Check host status
    end

    Note over Nixie: Return when all machines are installed
```
