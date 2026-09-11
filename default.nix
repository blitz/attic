let
  flake = import ./flake-compat.nix;
in flake.defaultNix.default.overrideAttrs (_: {
  passthru = {
    celler = flake.defaultNix.outputs.packages.${builtins.currentSystem}.celler;
  };
})
