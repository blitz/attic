{ pkgs, lib, config, flake, ... }:
let
  inherit (lib) types;

  serverConfigFile = config.nodes.server.services.cellerd.configFile;

  cmd = {
    make-token = "celler admin make-token --signing-key /etc/cellerd-secret --signing-algorithm hs256";
    cellerd = "cellerd -f ${serverConfigFile}";
  };

  makeTestDerivation = pkgs.writeShellScript "make-drv" ''
    name=$1
    base=$(basename $name)

    cat >$name <<EOF
    #!/bin/sh
    /*/sh -c "echo hello > \$out"; exit 0; */
    derivation {
      name = "$base";
      builder = ./$name;
      system = builtins.currentSystem;
      preferLocalBuild = true;
      allowSubstitutes = false;
    }
    EOF

    chmod +x $name
  '';

  databaseModules = {
    sqlite = {
      testScriptPost = ''
        from pathlib import Path
        import os

        schema = server.succeed("${pkgs.sqlite}/bin/sqlite3 /var/lib/cellerd/server.db '.schema --indent'")

        schema_path = Path(os.environ.get("out", os.getcwd())) / "schema.sql"
        with open(schema_path, 'w') as f:
            f.write(schema)
      '';
    };
    postgres = {
      server = {
        services.postgresql = {
          enable = true;
          ensureDatabases = [ "cellerd" ];
          ensureUsers = [
            {
              name = "cellerd";
              ensureDBOwnership = true;
            }

            # For testing only - Don't actually do this
            {
              name = "root";
              ensureClauses = {
                superuser = true;
              };
            }
          ];
        };

        services.cellerd.settings = {
          database.url = "postgresql:///cellerd?host=/run/postgresql";
        };
      };
      testScriptPost = ''
        from pathlib import Path
        import os

        schema = server.succeed("pg_dump --schema-only cellerd")

        schema_path = Path(os.environ.get("out", os.getcwd())) / "schema.sql"
        with open(schema_path, 'w') as f:
            f.write(schema)
      '';
    };
  };

  storageModules = {
    local = {};
    garage = let
      accessKey = "GKaaaaaaaaaaaaaaaaaaaaaaaa";
      secretKey = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    in {
      server = { pkgs, ...}: {
        services.garage = {
          enable = true;
          package = pkgs.garage_2;

          settings = {
            replication_factor = 1;
            consistent_mode = "consistent";

            rpc_bind_addr = "[::]:3901";
            rpc_public_addr = "[::]:3901";
            rpc_secret = "5c1915fa04d0b6739675c61bf5907eb0fe3d9c69850c83820f51b4d25d13868c";

            s3_api = {
              s3_region = "garage";
              api_bind_addr = "[::]:9000";
              root_domain = ".s3.garage";
            };

          };
        };

        networking.firewall.allowedTCPPorts = [ 9000 ];

        services.cellerd.settings = {
          storage = {
            type = "s3";
            endpoint = "http://server:9000";
            region = "garage";
            bucket = "celler";

            credentials = {
              access_key_id = accessKey;
              secret_access_key = secretKey;
            };
          };
        };
      };

      # Heavily inspired by the Nixpkgs atticd test.
      testScript = ''
        server.wait_for_unit("garage.service")
        server.wait_for_open_port(3901)

        # Create cluster
        node_id = server.succeed("garage status | tail -n1 | cut -d' ' -f1")
        server.succeed(f"garage layout assign -z dc1 -c 128M {node_id}")
        server.succeed("garage layout apply --version 1")

        # Create bucket
        server.succeed("garage bucket create celler")

        # Create access keys
        server.succeed("garage key import ${accessKey} ${secretKey} --yes")
        server.succeed("garage bucket allow --read --write --owner celler --key ${accessKey}")

        server.wait_for_open_port(9000)
      '';
    };
  };
in {
  options = {
    database = lib.mkOption {
      type = types.enum [ "sqlite" "postgres" ];
      default = "sqlite";
    };
    storage = lib.mkOption {
      type = types.enum [ "local" "garage" ];
      default = "local";
    };
  };

  config = {
    name = "basic-${config.database}-${config.storage}";

    nodes = {
      server = {
        imports = [
          flake.nixosModules.cellerd
          (databaseModules.${config.database}.server or {})
          (storageModules.${config.storage}.server or {})
        ];

        # For testing only. Don't put secrets into world-readable files.
        environment.etc."cellerd-secret".text = ''
          It doesn't matter what's in this file. It's used as the secret key for HS256 signing.
        '';

        services.cellerd = {
          enable = true;
          settings = {
            listen = "[::]:8080";

            jwt = {
              # For testing only. Don't put secrets into world-readable files.
              hs256-secret-key-file = "/etc/cellerd-secret";
            };

            chunking = {
              nar-size-threshold = 1;
              min-size = 64 * 1024;
              avg-size = 128 * 1024;
              max-size = 256 * 1024;
            };
          };
        };

        environment.systemPackages = [ pkgs.openssl pkgs.celler ];

        networking.firewall.allowedTCPPorts = [ 8080 ];
      };

      client = {
        environment.systemPackages = [ pkgs.celler ];

        # Otherwise the test log is spammed with
        # warning: error: unable to download 'https://cache.nixos.org/nix-cache-info': ...
        nix.settings.substituters = lib.mkForce [ ];
      };
    };

    testScript = ''
      import time

      start_all()

      ${databaseModules.${config.database}.testScript or ""}
      ${storageModules.${config.storage}.testScript or ""}

      server.wait_for_unit('cellerd.service')
      client.wait_until_succeeds("curl -sL http://server:8080", timeout=40)

      root_token = server.succeed("${cmd.make-token} --sub 'e2e-root' --validity '1 month' --push '*' --pull '*' --delete '*' --create-cache '*' --destroy-cache '*' --configure-cache '*' --configure-cache-retention '*' </dev/null").strip()
      readonly_token = server.succeed("${cmd.make-token} --sub 'e2e-root' --validity '1 month' --pull 'test' </dev/null").strip()

      client.succeed(f"celler login --set-default root http://server:8080 {root_token}")
      client.succeed(f"celler login readonly http://server:8080 {readonly_token}")
      client.succeed("celler login anon http://server:8080")

      # TODO: Make sure the correct status codes are returned
      # (i.e., 500s shouldn't pass the "should fail" tests)

      with subtest("Check that we can create a cache"):
          client.succeed("celler cache create test")

      with subtest("Check that we can push a path"):
          client.succeed("${makeTestDerivation} test.nix")
          test_file = client.succeed("nix-build --no-out-link test.nix").strip()
          test_file_hash = test_file.removeprefix("/nix/store/")[:32]

          client.succeed(f"celler push test {test_file}")
          client.succeed(f"nix-store --delete {test_file}")
          client.fail(f"ls {test_file}")

      with subtest("Check that we can pull a path"):
          client.succeed("celler use readonly:test")
          client.succeed(f"nix-store -r {test_file}")
          client.succeed(f"grep hello {test_file}")

      with subtest("Check that we cannot push without required permissions"):
          client.fail(f"celler push readonly:test {test_file}")
          client.fail(f"celler push anon:test {test_file} 2>&1")

      with subtest("Check that we can push a list of paths from stdin"):
          paths = []
          for i in range(10):
              client.succeed(f"${makeTestDerivation} seq{i}.nix")
              path = client.succeed(f"nix-build --no-out-link seq{i}.nix").strip()
              client.succeed(f"echo {path} >>paths.txt")
              paths.append(path)

          client.succeed("celler push test --stdin <paths.txt 2>&1")

          for path in paths:
              client.succeed(f"nix-store --delete {path}")

      with subtest("Check that we can pull the paths back"):
          for path in paths:
              client.fail(f"ls {path}")
              client.succeed(f"nix-store -r {path}")
              client.succeed(f"grep hello {path}")

      with subtest("Check that we can make the cache public"):
          client.fail("curl -sL --fail-with-body http://server:8080/test/nix-cache-info")
          client.fail(f"curl -sL --fail-with-body http://server:8080/test/{test_file_hash}.narinfo")
          client.succeed("celler cache configure test --public")
          client.succeed("curl -sL --fail-with-body http://server:8080/test/nix-cache-info")
          client.succeed(f"curl -sL --fail-with-body http://server:8080/test/{test_file_hash}.narinfo")

      with subtest("Check that we can trigger garbage collection"):
          test_file_hash = test_file.removeprefix("/nix/store/")[:32]
          client.succeed(f"curl -sL --fail-with-body http://server:8080/test/{test_file_hash}.narinfo")
          client.succeed("celler cache configure test --retention-period 1s")
          time.sleep(2)
          server.succeed("${cmd.cellerd} --mode garbage-collector-once")
          client.fail(f"curl -sL --fail-with-body http://server:8080/test/{test_file_hash}.narinfo")

      ${lib.optionalString (config.storage == "local") ''
      with subtest("Check that all chunks are actually deleted after GC"):
          files = server.succeed("find /var/lib/cellerd/storage -type f ! -name 'VERSION'")
          print(f"Remaining files: {files}")
          assert files.strip() == "", "Some files remain after GC: " + files
      ''}

      with subtest("Check that we can include the upload info in the payload"):
          client.succeed("${makeTestDerivation} test2.nix")
          test2_file = client.succeed("nix-build --no-out-link test2.nix")
          client.succeed(f"celler push --force-preamble test {test2_file}")
          client.succeed(f"nix-store --delete {test2_file}")
          client.succeed(f"nix-store -r {test2_file}")

      with subtest("Check that we can destroy the cache"):
          client.succeed("celler cache info test")
          client.succeed("celler cache destroy --no-confirm test")
          client.fail("celler cache info test")
          client.fail("curl -sL --fail-with-body http://server:8080/test/nix-cache-info")

      ${databaseModules.${config.database}.testScriptPost or ""}
      ${storageModules.${config.storage}.testScriptPost or ""}
    '';
  };
}
