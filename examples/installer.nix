{
  config,
  lib,
  modulesPath,
  pkgs,
  ...
}:

{
  imports = [
    (modulesPath + "/installer/netboot/netboot-minimal.nix")
  ];

  installer.cloneConfig = false;

  system.stateVersion = "25.05";

  # Optional size and build-time optimizations for the example and CI.
  boot = {
    supportedFilesystems = lib.mkForce [
      "ext4"
      "vfat"
    ];
    swraid.enable = lib.mkForce false;
  };

  documentation = {
    enable = lib.mkForce false;
    man.enable = lib.mkForce false;
    nixos.enable = lib.mkForce false;
  };

  netboot.squashfsCompression = "zstd -Xcompression-level 1";
  nix.registry = lib.mkForce { };
  security.polkit.enable = lib.mkForce false;
  security.sudo.enable = lib.mkForce false;
  system.installer.channel.enable = false;

  system.tools = {
    nixos-build-vms.enable = false;
    nixos-enter.enable = false;
    nixos-generate-config.enable = false;
    nixos-option.enable = false;
    nixos-rebuild.enable = false;
    nixos-version.enable = false;
  };

  system.build.netbootRamdisk = lib.mkForce (
    pkgs.makeInitrdNG {
      inherit (config.boot.initrd) compressor;
      compressorArgs = [ "-1" ];
      prepend = [ "${config.system.build.initialRamdisk}/initrd" ];

      contents = [
        {
          source = config.system.build.squashfsStore;
          target = "/nix-store.squashfs";
        }
      ];
    }
  );
}
