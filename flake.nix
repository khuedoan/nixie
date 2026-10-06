{
  description = "Nixie";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      system = "x86_64-linux";

      pkgs = import nixpkgs {
        inherit system;
      };

      nixieSrc = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [
          ./Cargo.toml
          ./Cargo.lock
          ./crates
        ];
      };

      nixie = pkgs.rustPlatform.buildRustPackage {
        pname = "nixie";
        version = "0.1";
        src = nixieSrc;
        cargoLock.lockFile = ./Cargo.lock;
        meta.mainProgram = "nixie";
      };

      python = pkgs.python3.withPackages (
        ps: with ps; [
          cryptography
        ]
      );

      e2eRunner = pkgs.writeShellApplication {
        name = "nixie-e2e";
        runtimeInputs = with pkgs; [
          dnsmasq
          git
          iproute2
          nix
          nixos-anywhere
          opentelemetry-collector
          openssh
          OVMF.fd
          python
          qemu_kvm
        ];
        text = ''
          export NIXIE_BIN="${nixie}/bin/nixie"
          export OVMF_CODE="${pkgs.OVMF.fd}/FV/OVMF_CODE.fd"
          export OVMF_VARS="${pkgs.OVMF.fd}/FV/OVMF_VARS.fd"
          exec ${python}/bin/python3 "${self.outPath}/tests/e2e.py" "$@"
        '';
      };
    in
    {
      packages.${system} = {
        default = nixie;
        nixie = nixie;
        nixie-agent = nixie;
      };

      apps.${system}.e2e = {
        type = "app";
        program = "${e2eRunner}/bin/nixie-e2e";
      };

      nixosModules.nixie-agent =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        {
          systemd.services.nixie-agent = {
            description = "Nixie Agent";
            wantedBy = [ "multi-user.target" ];
            serviceConfig = {
              ExecStart = "${self.packages.${system}."nixie-agent"}/bin/nixie-agent";
              Restart = "on-failure";
            };
          };
        };

      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cargo
          pkgs.clippy
          pkgs.rustc
          pkgs.rustfmt
          pkgs.gnumake
          pkgs.nixfmt-tree
          # TODO maybe embed this into the binary?
          pkgs.nixos-anywhere
        ];
      };
    };
}
