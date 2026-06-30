{ lib, stdenv, runCommand, celler, ansi2html }:

with builtins;

let
  commands = {
    celler = [
      null
      "login"
      "use"
      "push"
      "watch-store"
      "cache"
      "cache create"
      "cache configure"
      "cache destroy"
      "cache info"
      "admin make-token"
    ];
    cellerd = [
      null
    ];
  };
  renderMarkdown = name: subcommands: ''
    mkdir -p $out
    (
      ansi2html -H
      ${lib.concatMapStrings (subcommand: let
        fullCommand = "${name} ${if subcommand == null then "" else subcommand}";
      in "${renderCommand fullCommand}\n") subcommands}
    ) >>$out/${name}.md
  '';
  renderCommand = fullCommand: ''
    echo '## `${fullCommand}`'
    echo -n '<pre><div class="hljs">'
    TERM=xterm-256color CLICOLOR_FORCE=1 ${fullCommand} --help | ansi2html -p
    echo '</div></pre>'
  '';
in runCommand "celler-colorized-help" {
  nativeBuildInputs = [ celler ansi2html ];
} (concatStringsSep "\n" (lib.mapAttrsToList renderMarkdown commands))
