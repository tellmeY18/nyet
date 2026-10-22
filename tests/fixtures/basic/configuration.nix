{ ... }:
{
  system.stateVersion = "24.05";

  networking.hostName = "nyet-fixture";

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
  };

  boot.loader.grub.devices = [ "/dev/sda" ];

  services.openssh.enable = true;

  environment.systemPackages = [ ];
}
