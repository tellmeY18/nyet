{ config, lib, pkgs, ... }:
let
  cfg = config.programs.nyet;
in {
  options.programs.nyet = {
    enable = lib.mkEnableOption "nyet, the NixOS configuration engine";
    package = lib.mkOption {
      type = lib.types.package;
      description = "The nyet package providing the nyet and nyet-helper binaries.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    security.polkit.enable = true;
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.freedesktop.policykit.exec" &&
            (action.lookup("program") == "${cfg.package}/bin/nyet-helper" ||
             action.lookup("program") == "/run/current-system/sw/bin/nyet-helper")) {
          return polkit.Result.AUTH_ADMIN;
        }
        return polkit.Result.NOT_HANDLED;
      });
    '';
  };
}
