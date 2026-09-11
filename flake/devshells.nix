# Development shells

toplevel @ { lib, flake-parts-lib, ... }:
let
  inherit (lib)
    mkOption
    types
    ;
  inherit (flake-parts-lib)
    mkPerSystemOption
    ;
in
{
  options = {
    perSystem = mkPerSystemOption {
      options.celler.devshell = {
        packageSets = mkOption {
          type = types.attrsOf (types.listOf types.package);
          default = {};
        };
        extraPackages = mkOption {
          type = types.listOf types.package;
          default = [];
        };
        extraArgs = mkOption {
          type = types.attrsOf types.unspecified;
          default = {};
        };
      };
    };
  };

  config = {
    perSystem = { self', pkgs, config, ... }: let
      cfg = config.celler.devshell;
    in {
      celler.devshell.packageSets = with pkgs; {
        rust = [
          rustc
          cargo-audit
          cargo-expand
          cargo-outdated
          cargo-edit
          cargo-udeps
          tokio-console
        ];

        linters = [
          clippy
          rustfmt

          editorconfig-checker
        ];

        ops = [
          postgresql
          sqlite-interactive
        ];

        bench = [
          wrk
        ] ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          perf
        ];

      };

      devShells.default = pkgs.mkShell (lib.recursiveUpdate {
        inputsFrom = [
          self'.packages.celler
          self'.packages.book
        ];

        packages = lib.flatten (lib.attrValues cfg.packageSets);

        env = {
          CELLER_DISTRIBUTOR = toplevel.config.celler.distributor;

          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;

          NIX_PATH = "nixpkgs=${pkgs.path}";
        };
      } cfg.extraArgs);
    };
  };
}
