{
  lib,
  pkgs,
  config,
  ...
}:

let
  inherit (lib) types;

  cfg = config.services.cellerd;

  # unused when the entrypoint is flake
  flake = import ../flake-compat.nix;
  overlay = flake.defaultNix.overlays.default;

  format = pkgs.formats.toml { };

  hasLocalPostgresDB =
    let
      url = cfg.settings.database.url or "";
      localStrings = [
        "localhost"
        "127.0.0.1"
        "/run/postgresql"
      ];
      hasLocalStrings = lib.any (lib.flip lib.hasInfix url) localStrings;
    in
    config.services.postgresql.enable && lib.hasPrefix "postgresql://" url && hasLocalStrings;
in
{
  options = {
    services.cellerd = {
      enable = lib.mkEnableOption "the cellerd, the Nix Binary Cache server";

      package = lib.mkPackageOption pkgs "celler" { };

      environmentFile = lib.mkOption {
        description = ''
          Path to an EnvironmentFile containing required environment
          variables. This is usually used to provide S3 credentials.

          NOTE: JWT secrets can only be provided via services.cellerd.settings.jwt.
          See the documentation for details.
        '';
        type = lib.types.nullOr lib.types.externalPath;
        default = null;
      };

      user = lib.mkOption {
        description = ''
          The user under which celler runs.
        '';
        type = types.str;
        default = "cellerd";
      };

      group = lib.mkOption {
        description = ''
          The group under which celler runs.
        '';
        type = types.str;
        default = "cellerd";
      };

      settings = lib.mkOption {
        description = ''
          Structured configurations of cellerd.
        '';
        type = format.type;
        default = { }; # setting defaults here does not compose well
      };

      configFile = lib.mkOption {
        description = ''
          Path to an existing cellerd configuration file.

          By default, it's generated from `services.cellerd.settings`.
        '';
        type = types.path;
        default = format.generate "server.toml" cfg.settings;
        defaultText = "generated from `services.cellerd.settings`";
      };

      mode = lib.mkOption {
        description = ''
          Mode in which to run the server.

          'monolithic' runs all components, and is suitable for single-node deployments.

          'api-server' runs only the API server, and is suitable for clustering.

          'garbage-collector' only runs the garbage collector periodically.

          A simple NixOS-based Celler deployment will typically have one 'monolithic' and any number of 'api-server' nodes.

          There are several other supported modes that perform one-off operations, but these are the only ones that make sense to run via the NixOS module.
        '';
        type = lib.types.enum [
          "monolithic"
          "api-server"
          "garbage-collector"
        ];
        default = "monolithic";
      };

      # Internal flags
      useFlakeCompatOverlay = lib.mkOption {
        description = ''
          Whether to insert the overlay with flake-compat.
        '';
        type = types.bool;
        internal = true;
        default = true;
      };
    };
  };

  config = lib.mkIf cfg.enable {
    services.cellerd.settings = {
      database.url = lib.mkDefault "sqlite:///var/lib/cellerd/server.db?mode=rwc";

      # "storage" is internally tagged
      # if the user sets something the whole thing must be replaced
      storage = lib.mkDefault {
        type = "local";
        path = "/var/lib/cellerd/storage";
      };
    };

    systemd.services.cellerd = {
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ] ++ lib.optionals hasLocalPostgresDB [ "postgresql.service" ];
      requires = lib.optionals hasLocalPostgresDB [ "postgresql.service" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/cellerd -f ${cfg.configFile} --mode ${cfg.mode}";
        EnvironmentFile = cfg.environmentFile;
        StateDirectory = "cellerd"; # for usage with local storage and sqlite
        DynamicUser = true;
        User = cfg.user;
        Group = cfg.group;
        Restart = "on-failure";
        RestartSec = 10;

        CapabilityBoundingSet = [ "" ];
        DeviceAllow = "";
        DevicePolicy = "closed";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        PrivateUsers = true;
        ProcSubset = "pid";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "strict";
        ReadWritePaths =
          let
            path = cfg.settings.storage.path;
            isDefaultStateDirectory = path == "/var/lib/cellerd" || lib.hasPrefix "/var/lib/cellerd/" path;
          in
          lib.optionals (cfg.settings.storage.type or "" == "local" && !isDefaultStateDirectory) [ path ];
        RemoveIPC = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@resources"
          "~@privileged"
        ];
        UMask = "0077";
      };
    };

    nixpkgs.overlays = lib.mkIf cfg.useFlakeCompatOverlay [
      overlay
    ];
  };
}
