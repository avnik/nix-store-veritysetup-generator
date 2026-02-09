{ self, ... }:
{
  flake.nixosModules.ghaf-store-veritysetup-generator =
    {
      config,
      lib,
      pkgs,
      ...
    }:

    let
      cfg = config.boot.initrd.systemd.ghaf-store-veritysetup-generator;
      generator = self.packages.${pkgs.hostPlatform.system}.ghaf-store-veritysetup-generator.override {
        # Use exact systemd that we use in boot, because paths to tools become hardcoded into executable
        # and `storePath` trick stop working
        systemd = config.boot.initrd.systemd.package;
        inherit (cfg)
          nixStoreVolumeName
          volumeGroupName
          ghafStorehashArgName
          ghafRevisionArgName
          ;
      };

    in

    {

      options.boot.initrd.systemd.ghaf-store-veritysetup-generator = {

        enable = lib.mkEnableOption "ghaf-store-veritysetup-generator";
        package = lib.mkOption {
          description = "Package of ghaf-store-veritysetup-generator";
          type = lib.types.package;
          default = generator;
        };

        volumeGroupName = lib.mkOption {
          type = lib.types.str;
          default = "pool";
          description = ''
            Name of LUKS/LVM volume group to operate on
            (would be compiled into binary)
          '';
        };
        nixStoreVolumeName = lib.mkOption {
          type = lib.types.str;
          default = "nix-store";
          description = ''
            Name of volume for /nix/store inside /dev/mapper/* 
            (would be compiled into binary)
          '';
        };
        ghafStorehashArgName = lib.mkOption {
          type = lib.types.str;
          default = "ghaf.storehash";
          description = ''
            Name kernel cmdline parameter for store verity hash. 
            (would be compiled into binary)
          '';
        };
        ghafRevisionArgName = lib.mkOption {
          type = lib.types.str;
          default = "ghaf.revision";
          description = ''
            Name of kernel cmdline for volume revision 
            (would be compiled into binary)
          '';
        };

      };

      config = lib.mkIf cfg.enable {

        assertions = [
          {
            assertion = config.boot.initrd.systemd.enable;
            message = "ghaf-store-veritysetup-generator only works in the systemd initrd.";
          }
        ];

        boot.initrd.systemd = {

          contents = {
            "/etc/systemd/system-generators/ghaf-store-veritysetup-generator".source = "${cfg.package}/bin/nix-store-veritysetup-generator";
          };

          storePaths = [
            "${config.boot.initrd.systemd.package}/bin/systemd-escape"
          ];

        };
      };
    };
}
