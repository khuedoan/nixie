{
  description = "Nixie";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
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

      app = pkgs.buildGoApplication {
        pname = "nixie";
        version = "0.1";
        src = ./.;
        modules = ./gomod2nix.toml;
      };

      goEnv = pkgs.mkGoEnv { pwd = ./.; };
    in
    {
      packages.${system}.default = app;

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
