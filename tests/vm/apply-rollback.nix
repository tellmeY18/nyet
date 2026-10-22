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
    system.switch.enable = true;
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
      pkgs.nh
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
              system.switch.enable = true;
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
                pkgs.nh
              ];
            })
          ];
        };
      };
    }
  '';

  classicConfiguration = pkgs.writeText "classic-configuration.nix" ''
    { pkgs, ... }:
    {
      imports = [
        ./hardware-configuration.nix
        (import "${nixpkgsPath}/nixos/lib/testing/nixos-test-base.nix")
        (import "${nixpkgsPath}/nixos/modules/virtualisation/qemu-vm.nix")
      ];
      system.stateVersion = "24.05";
      system.switch.enable = true;
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
        pkgs.nh
      ];
      networking.hostName = "nyet-applied";
    }
  '';

  classicHardware = pkgs.writeText "classic-hardware.nix" ''
    { ... }:
    { }
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
    system.switch.enable = true;
    users.users.alice = { isNormalUser = true; createHome = true; };
    boot.loader.grub.enable = false;

    environment.systemPackages = [ pkgs.nix pkgs.nixos-rebuild pkgs.nh ];

    nix.settings.experimental-features = [ "nix-command" "flakes" ];
    nix.settings.substituters = [ ];

    virtualisation = {
      memorySize = 2048;
      cores = 2;
      additionalPaths = [
        flakeFile
        classicConfiguration
        classicHardware
        nyet
        nixpkgsPath
        pkgs.path
        pkgs.nix
        pkgs.nixos-rebuild
        pkgs.nh
        target.config.system.build.toplevel
        target.config.system.build.nixos-rebuild
      ];
    };
  };

  testScript = ''
    start_all()
    machine.wait_for_unit("multi-user.target")

    machine.succeed("install -d /etc/nixos")
    machine.succeed("install -Dm644 ${classicConfiguration} /etc/nixos/configuration.nix")
    machine.succeed("install -Dm644 ${classicHardware} /etc/nixos/hardware-configuration.nix")
    machine.succeed("nyet --flake /etc/nixos --host machine migration-plan --system x86_64-linux --nixpkgs path:${nixpkgsPath}")
    machine.succeed("nyet --flake /etc/nixos --host machine init --system x86_64-linux --nixpkgs path:${nixpkgsPath} --yes", timeout=1800)
    machine.succeed("test -f /etc/nixos/hosts/machine/configuration.nix")
    machine.succeed("test -f /etc/nixos/hosts/machine/hardware-configuration.nix")
    machine.succeed("test -f /etc/nixos/flake.nix")
    machine.succeed("grep -Fq 'url = ' /etc/nixos/flake.nix")
    machine.succeed("grep -Fq '${nixpkgsPath}' /etc/nixos/flake.nix")
    machine.succeed("test ! -e /etc/nixos/configuration.nix")
    machine.succeed("nix build --no-link --no-write-lock-file /etc/nixos#machine.config.system.build.toplevel", timeout=1800)

    import json
    history = json.loads(machine.succeed("nyet history --json"))
    migrations = [entry for entry in history if entry["kind"] == "migration"]
    snapshot = migrations[-1]["plan_id"]
    machine.succeed(f"nyet migration-rollback {snapshot} --yes", timeout=1800)
    machine.succeed("test -f /etc/nixos/configuration.nix")
    machine.succeed("test -f /etc/nixos/hardware-configuration.nix")
    machine.succeed("test ! -e /etc/nixos/flake.nix")
    machine.succeed("test ! -e /etc/nixos/flake.lock")
    machine.succeed("test ! -e /etc/nixos/hosts")

    machine.succeed("install -Dm644 ${flakeFile} /etc/nixos/flake.nix")
    machine.succeed("nyet --flake /etc/nixos --host machine set networking.hostName nyet-applied")
    machine.succeed("nix-env -p /nix/var/nix/profiles/system --set /run/current-system")
    machine.succeed("nyet --flake /etc/nixos --host machine plan --check")
    machine.succeed("nyet --flake /etc/nixos --host machine apply --yes", timeout=1800)

    machine.succeed("grep -q nyet-applied /etc/hostname")
    machine.succeed("grep -q 'networking.hostName = \"nyet-applied\"' /var/lib/nyet/nyet.nix")
    machine.succeed("nyet history --json | grep -q '\"kind\": \"apply\"'")

    machine.succeed("nyet rollback --yes", timeout=1800)

    machine.succeed("grep -q machine /etc/hostname")
    machine.succeed("test ! -e /var/lib/nyet/nyet.nix")
    machine.succeed("nyet history --json | grep -q '\"kind\": \"rollback\"'")

    import base64, json, time

    plan_id = "0123456789abcdef0123456789abcdef"
    now = int(time.time())
    module = "{ config, lib, pkgs, ... }:" + chr(10) + "{" + chr(10) + '  networking.hostName = "nyet-applied";' + chr(10) + "}" + chr(10)
    plan = {
      "version": 1,
      "id": plan_id,
      "uid": 1000,
      "host": "machine",
      "created": now,
      "expires": now + 600,
      "module": module,
    }
    encoded = base64.b64encode(json.dumps(plan).encode()).decode()

    machine.succeed("install -d -o alice -g users /home/alice /run/user/1000/nyet/plans")
    machine.succeed(f"printf %s {encoded} | base64 -d > /run/user/1000/nyet/plans/{plan_id}.json")
    machine.succeed(f"chown alice:users /run/user/1000/nyet/plans/{plan_id}.json && chmod 600 /run/user/1000/nyet/plans/{plan_id}.json")
    machine.succeed(f"PKEXEC_UID=1000 /run/current-system/sw/bin/nyet-helper apply {plan_id}", timeout=1800)

    machine.succeed("grep -q nyet-applied /etc/hostname")
    machine.succeed("grep -q 'networking.hostName = \"nyet-applied\"' /var/lib/nyet/nyet.nix")

    machine.succeed("nyet rollback --yes", timeout=1800)

    machine.succeed("grep -q machine /etc/hostname")
    machine.succeed("test ! -e /var/lib/nyet/nyet.nix")
  '';
}
