{ flake, host, module }:
let
  f = builtins.getFlake (toString flake);
  cfg = f.nixosConfigurations.${host};
  extended = cfg.extendModules {
    modules = [ (import (builtins.toFile "nyet-managed.nix" module)) ];
  };
in
extended.config.system.build.toplevel.drvPath
