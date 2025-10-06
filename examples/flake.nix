{
  inputs = {
    nixpkgs = {
      url = "github:nixos/nixpkgs/nixos-25.05";
    };
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      disko,
    }:
    {
      nixosConfigurations = {
        installer = nixpkgs.lib.nixosSystem {
          system = "x86_64-linux";
          modules = [
            ./installer.nix
          ];
        };
        machine1 = nixpkgs.lib.nixosSystem {
          system = "x86_64-linux";
          modules = [
            disko.nixosModules.disko
            ./configuration.nix
            {
              networking.hostName = "machine1";
            }
          ];
        };
      };
    };
}
