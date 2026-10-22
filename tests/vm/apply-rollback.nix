{ pkgs, self }:
let
  nyet = self.packages.x86_64-linux.default;
  nyetPath = builtins.unsafeDiscardStringContext (toString nyet);
  nixpkgsPath = self.inputs.nixpkgs.outPath;

  baseModule = { pkgs, ... }: {
    imports = [
      (import "${nixpkgsPath}/nixos/lib/testing/nixos-test-base.nix")
      (import "${nixpkgsPath}/nixos/modules/virtualisation/qemu-vm.nix")
    ];
    system.stateVersion = "24.05";
    virtualisation.host.pkgs = pkgs;
    fileSystems."/" = { device = "/dev/vda"; fsType = "ext4"; };
    boot.loader.grub.enable = false;
    nix.settings.experimental-features = [ "nix-command" "flakes" ];
    nix.settings.substituters = [ ];
    environment.systemPackages = [
      (pkgs.writeShellScriptBin "nyet" "exec ${nyetPath}/bin/nyet $@")
      (pkgs.writeShellScriptBin "nyet-helper" "exec ${nyetPath}/bin/nyet-helper $@")
      pkgs.nixos-rebuild
      pkgs.nix
    ];
  };

  flakeFile = pkgs.writeText "nixos-flake.nix" ''
    {
      inputs.nixpkgs.url = "path:${nixpkgsPath}";
      outputs = { self, nixpkgs }: {
        nixosConfigurations.machine = nixpkgs.lib.nixosSystem {
          modules = [
            { nixpkgs.hostPlatform = "x86_64-linux"; }
            ({ pkgs, ... }: {
              imports = [
                (import "${nixpkgsPath}/nixos/lib/testing/nixos-test-base.nix")
                (import "${nixpkgsPath}/nixos/modules/virtualisation/qemu-vm.nix")
              ];
              system.stateVersion = "24.05";
              virtualisation.host.pkgs = pkgs;
              fileSystems."/" = { device = "/dev/vda"; fsType = "ext4"; };
              boot.loader.grub.enable = false;
              nix.settings.experimental-features = [ "nix-command" "flakes" ];
              nix.settings.substituters = [ ];
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

  target = self.inputs.nixpkgs.lib.nixosSystem {
    modules = [
      { nixpkgs.hostPlatform = "x86_64-linux"; }
      baseModule
      { networking.hostName = "nyet-applied"; }
    ];
  };
in
pkgs.testers.runNixOSTest {
  name = "nyet-apply-rollback";
  requiredFeatures.kvm = false;
  nodes.machine = { pkgs, ... }: {
    imports = [ self.nixosModules.default ];

    programs.nyet.enable = true;
    networking.hostName = "machine";

    environment.systemPackages = [ pkgs.nix pkgs.nixos-rebuild ];

    nix.settings.experimental-features = [ "nix-command" "flakes" ];
    nix.settings.substituters = [ ];

    virtualisation = {
      memorySize = 2048;
      cores = 2;
      additionalPaths = [
        flakeFile
        nyet
        nixpkgsPath
        pkgs.path
        pkgs.nix
        pkgs.nixos-rebuild
        target.config.system.build.toplevel
        target.config.system.build.nixos-rebuild
      ];
    };
  };

  testScript = ''
    start_all()
    machine.wait_for_unit("multi-user.target")

    machine.succeed("install -Dm644 ${flakeFile} /etc/nixos/flake.nix")
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
