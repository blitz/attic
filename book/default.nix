{ lib, stdenv, nix-gitignore, mdbook, mdbook-linkcheck, python3, callPackage, writeScript
, celler ? null
}:

let
  colorizedHelp = let
    help = callPackage ./colorized-help.nix {
      inherit celler;
    };
  in if celler != null then help else null;
in stdenv.mkDerivation {
  inherit colorizedHelp;

  name = "celler-book";

  src = nix-gitignore.gitignoreSource [] ./.;

  nativeBuildInputs = [ mdbook ];

  buildPhase = ''
    emitColorizedHelp() {
      command=$1

      if [[ -n "$colorizedHelp" ]]; then
          cat "$colorizedHelp/$command.md" >> src/reference/$command-cli.md
      else
          echo "Error: No celler executable passed to the builder" >> src/reference/$command-cli.md
      fi
    }

    emitColorizedHelp celler
    emitColorizedHelp cellerd

    mdbook build -d ./build
    cp -r ./build $out
  '';

  installPhase = "true";
}
