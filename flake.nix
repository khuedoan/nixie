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

      goEnv = pkgs.mkGoEnv { pwd = ./.; };
    in
    {
      packages.${system} = {
        default = app;
        nixie-agent = agent;
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
