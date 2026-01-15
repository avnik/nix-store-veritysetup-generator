{
  config,
  lib,
  pkgs,
  ...
}:

let

  cfg = config.boot.initrd.systemd.ghaf-store-veritysetup-generator;
  generator = pkgs.ghaf-store-veritysetup-generator.override {
    # Use exact systemd that we use in boot, because paths to tools become hardcoded into executable
    systemd = config.boot.initrd.systemd.package;
  };

in

{

  options.boot.initrd.systemd.ghaf-store-veritysetup-generator = {

    enable = lib.mkEnableOption "ghaf-store-veritysetup-generator";

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
        "/etc/systemd/system-generators/ghaf-store-veritysetup-generator".source = "${pkgs.ghaf-store-veritysetup-generator}/bin/nix-store-veritysetup-generator";
      };

      storePaths = [
        "${config.boot.initrd.systemd.package}/bin/systemd-escape"
      ];

    };

  };

}
