{ lib, modulesPath, ... }:

{
  imports = [
    (modulesPath + "/installer/netboot/netboot-minimal.nix")
  ];

  networking.hostName = "nixos-installer";
  services.openssh.enable = true;

  users.users.root = {
    password = "nixos-installer";
    initialHashedPassword = lib.mkForce null;
  };

  system.stateVersion = "25.05";
}
