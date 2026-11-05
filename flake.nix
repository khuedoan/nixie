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
        ./internal/protocol
      ];

      app = pkgs.buildGoApplication {
        pname = "nixie";
        version = "0.1";
        src = appSource;
        modules = ./gomod2nix.toml;
        subPackages = [
          "./cmd/nixie"
          "./internal/..."
        ];
        nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
        postFixup = ''
          wrapProgram "$out/bin/nixie" \
            --prefix PATH : ${
              pkgs.lib.makeBinPath [
                pkgs.nix
                pkgs.gnutar
                pkgs.gzip
              ]
            }
        '';
      };

      agent = pkgs.buildGoApplication {
        pname = "nixie-agent";
        version = "0.1";
        src = agentSource;
        modules = ./gomod2nix.toml;
        subPackages = [ "./cmd/nixie-agent" ];
        nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
        postFixup = ''
          wrapProgram "$out/bin/nixie-agent" \
            --prefix PATH : ${pkgs.lib.makeBinPath [
              pkgs.nix
              pkgs.gitMinimal
              pkgs.nixos-install-tools
              pkgs.util-linux
              pkgs.systemd
            ]}
        '';
      };

      e2eRunner = pkgs.writeShellApplication {
        name = "nixie-e2e";
        runtimeInputs = with pkgs; [
          dnsmasq
          git
          iproute2
          iptables
          nix
          opentelemetry-collector
          openssh
          OVMF.fd
          python3
          qemu_kvm
        ];
        text = ''
          export NIXIE_BIN="${app}/bin/nixie"
          export OVMF_CODE="${pkgs.OVMF.fd}/FV/OVMF_CODE.fd"
          export OVMF_VARS="${pkgs.OVMF.fd}/FV/OVMF_VARS.fd"
          exec ${pkgs.python3}/bin/python3 "${self.outPath}/tests/e2e.py" "$@"
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
          pkgs,
          ...
        }:
        {
          system.build.nixieInstaller = pkgs.linkFarm "nixie-installer" [
            {
              name = "kernel";
              path = "${config.system.build.kernel}/bzImage";
            }
            {
              name = "initrd";
              path = "${config.system.build.netbootRamdisk}/initrd";
            }
            {
              name = "init";
              path = "${config.system.build.toplevel}/init";
            }
          ];
          systemd.services.nixie-agent = {
            description = "Nixie Agent";
            wantedBy = [ "multi-user.target" ];
            wants = [ "network-online.target" ];
            after = [ "network-online.target" ];
            serviceConfig = {
              ExecStart = "${self.packages.${system}."nixie-agent"}/bin/nixie-agent";
              Restart = "no";
              User = "root";
              StandardOutput = "journal+console";
            };
          };
        };

      devShells.${system}.default = pkgs.mkShell {
        packages = [
          goEnv
          pkgs.gomod2nix
          pkgs.gnumake
          pkgs.nixfmt-tree
          pkgs.nix
          pkgs.openssh
        ];
      };
    };
}
