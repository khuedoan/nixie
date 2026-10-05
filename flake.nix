{
  description = "Nixie";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    gomod2nix = {
      url = "github:nix-community/gomod2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      gomod2nix,
    }:
    let
      system = "x86_64-linux";

      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          (import "${gomod2nix}/overlay.nix")
        ];
      };

      mkGoSource =
        fileset:
        pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions (
            fileset
            ++ [
              ./go.mod
              ./go.sum
              ./gomod2nix.toml
            ]
          );
        };

      appSource = mkGoSource [
        ./cmd/nixie
        ./internal
      ];

      agentSource = mkGoSource [
        ./cmd/nixie-agent
      ];

      mkGoPackage =
        { pname, src, subPackages }:
        pkgs.buildGoApplication {
          inherit pname subPackages;
          version = "0.1";
          inherit src;
          modules = ./gomod2nix.toml;
        };

      app = mkGoPackage {
        pname = "nixie";
        src = appSource;
        subPackages = [ "./cmd/nixie" ];
      };

      agent = mkGoPackage {
        pname = "nixie-agent";
        src = agentSource;
        subPackages = [ "./cmd/nixie-agent" ];
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
          export NIXIE_BIN="${app}/bin/nixie"
          export OVMF_CODE="${pkgs.OVMF.fd}/FV/OVMF_CODE.fd"
          export OVMF_VARS="${pkgs.OVMF.fd}/FV/OVMF_VARS.fd"
          exec ${python}/bin/python3 "${self.outPath}/tests/e2e.py" "$@"
        '';
      };

      goEnv = pkgs.mkGoEnv { pwd = ./.; };
    in
    {
      packages.${system} = {
        default = app;
        nixie-agent = agent;
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
          goEnv
          pkgs.gomod2nix
          pkgs.gnumake
          pkgs.nixfmt-tree
          # TODO maybe embed this into the binary?
          pkgs.nixos-anywhere
        ];
      };
    };
}
