{ lib, flake-parts-lib, self, inputs, ... }:
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
      options.celler.integration-tests = {
        nixpkgsArgs = mkOption {
          type = types.attrsOf types.anything;
          default = {};
        };
        tests = mkOption {
          type = types.attrsOf types.package;
          default = {};
        };
        stableTests = mkOption {
          type = types.attrsOf types.package;
          default = {};
        };
      };
    };
  };

  config = {
    perSystem = { self', pkgs, config, system, ... }: let
      cfg = config.celler.integration-tests;

      vmPkgs = import inputs.nixpkgs ({
        inherit system;
        overlays = [ self.overlays.default ];
      } // cfg.nixpkgsArgs);
      vmPkgsStable = import inputs.nixpkgs-stable ({
        inherit system;
        overlays = [ self.overlays.default ];
      } // cfg.nixpkgsArgs);

      makeIntegrationTests = pkgs: import ../integration-tests {
        inherit pkgs;
        flake = self;
      };
    in {
      celler.integration-tests = {
        tests = makeIntegrationTests vmPkgs;
        stableTests = makeIntegrationTests vmPkgsStable;
      };

      checks = let
        tests = cfg.tests;
        stableTests = lib.mapAttrs' (name: lib.nameValuePair "stable-${name}") cfg.stableTests;
      in lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (tests // stableTests);
    };
  };
}
