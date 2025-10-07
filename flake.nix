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

      go-test = pkgs.stdenvNoCC.mkDerivation {
        name = "go-test";
        src = ./.;
        dontBuild = true;
        doCheck = true;
        nativeBuildInputs = with pkgs; [
          go
          writableTmpDirAsHomeHook
        ];
        checkPhase = ''
          go test -v ./...
        '';
        installPhase = ''
          mkdir "$out"
        '';
      };

      go-lint = pkgs.stdenvNoCC.mkDerivation {
        name = "go-lint";
        src = ./.;
        dontBuild = true;
        doCheck = true;
        nativeBuildInputs = with pkgs; [
          golangci-lint
          go
          writableTmpDirAsHomeHook
        ];
        checkPhase = ''
          golangci-lint run
        '';
        installPhase = ''
          mkdir "$out"
        '';
      };

      goEnv = pkgs.mkGoEnv { pwd = ./.; };
    in
    {
      packages.${system}.default = app;

      checks.${system} = {
        inherit go-test go-lint;
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
