{ pkgs, self }:
let
  nyet = self.packages.x86_64-linux.default;

  flakeFile = pkgs.writeText "nixos-flake.nix" ''
    {
      inputs.nixpkgs.url = "path:${pkgs.path}";
      outputs = { self, nixpkgs }: {
        nixosConfigurations.machine = nixpkgs.lib.nixosSystem {
          modules = [
            { nixpkgs.hostPlatform = "x86_64-linux"; }
            ({ pkgs, ... }: {
              system.stateVersion = "24.05";
              fileSystems."/" = { device = "/dev/vda"; fsType = "ext4"; };
              boot.loader.grub.enable = false;
              networking.nameservers = [ "10.0.2.3" ];
              nix.settings.experimental-features = [ "nix-command" "flakes" ];
              nix.settings.substituters = [ "https://cache.nixos.org" ];
              environment.systemPackages = [
                (pkgs.writeShellScriptBin "nyet" "exec ${nyet}/bin/nyet $@")
                (pkgs.writeShellScriptBin "nyet-helper" "exec ${nyet}/bin/nyet-helper $@")
                pkgs.nixos-rebuild
                pkgs.nix
              ];
            })
          ];
        };
      };
    }
  '';
in
pkgs.testers.runNixOSTest {
  name = "nyet-apply-rollback";
  requiredFeatures.kvm = false;
  nodes.machine = { pkgs, ... }: {
    imports = [ self.nixosModules.default ];

    programs.nyet.enable = true;
    networking.hostName = "machine";
    networking.nameservers = [ "10.0.2.3" ];

    environment.systemPackages = [ pkgs.nix pkgs.nixos-rebuild ];

    nix.settings.experimental-features = [ "nix-command" "flakes" ];
    nix.settings.substituters = [ "https://cache.nixos.org" ];

    virtualisation = {
      memorySize = 2048;
      cores = 2;
      additionalPaths = [ flakeFile nyet pkgs.path pkgs.nix pkgs.nixos-rebuild ];
    };
  };

  testScript = ''
    start_all()
    machine.wait_for_unit("multi-user.target")

    machine.succeed("install -Dm644 ${flakeFile} /etc/nixos/flake.nix")
    machine.succeed("getent hosts cache.nixos.org")
    machine.succeed("nyet --flake /etc/nixos --host machine set networking.hostName nyet-applied")
    machine.succeed("nyet --flake /etc/nixos --host machine plan --check")
    machine.succeed("nyet --flake /etc/nixos --host machine apply --yes", timeout=1800)

    machine.succeed("grep -q nyet-applied /etc/hostname")
    machine.succeed("grep -q 'networking.hostName = \"nyet-applied\"' /var/lib/nyet/nyet.nix")
    machine.succeed("nyet history --json | grep -q '\"kind\": \"apply\"'")

    machine.succeed("nyet rollback --yes", timeout=1800)

    machine.succeed("grep -q machine /etc/hostname")
    machine.succeed("test ! -e /var/lib/nyet/nyet.nix")
    machine.succeed("nyet history --json | grep -q '\"kind\": \"rollback\"'")
  '';
}
