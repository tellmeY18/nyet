{ config, lib, pkgs, ... }:
{
  networking.hostName = "nyet";
  services.openssh.enable = true;
}
