{
  inputs = {
    nixpkgs = {
      url = "github:nixos/nixpkgs/nixos-25.05";
    };
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixie = {
      # TODO change this to a remote URL if you're building a custom installer
      # url = "github:khuedoan/nixie";
      url = "path:..";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      disko,
      nixie,
    }:
    {
      nixosConfigurations = {
        installer = nixpkgs.lib.nixosSystem {
          system = "x86_64-linux";
          modules = [
            ./installer.nix
            nixie.nixosModules.nixie-agent
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
        machine2 = nixpkgs.lib.nixosSystem {
          system = "x86_64-linux";
          modules = [
            disko.nixosModules.disko
            ./configuration.nix
            {
              networking.hostName = "machine2";
            }
          ];
        };
      };
    };
}
