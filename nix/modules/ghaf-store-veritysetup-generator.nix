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
