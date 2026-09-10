# Deploying to NixOS

Celler provides [a NixOS module](https://github.com/blitz/celler/blob/main/nixos/cellerd.nix) that allows you to deploy the Celler Server on a NixOS machine.

## Prerequisites

1. A machine running NixOS
1. _(Optional)_ A dedicated bucket on S3 or a S3-compatible storage service
    - You can either [set up Garage](https://search.nixos.org/options?query=services.garage) or use a hosted service like [Backblaze B2](https://www.backblaze.com/b2/docs) and [Cloudflare R2](https://developers.cloudflare.com/r2).
1. _(Optional)_ A PostgreSQL database

## Generating the Credentials File

The RS256 JWT secret can be generated with the `openssl` utility:

```bash
$ nix shell nixpkgs#openssl
$ openssl genrsa 4096 -out private-key.pem
$ openssl rsa -in private-key.pem -pubout -out public-key.pem
```

Keep the private-key.pem file in a secure location. You need it to sign tokens using `celler admin make-token`.

The public key will be part of the server configuration.

## Importing the Module

You can import the module in one of two ways:

- Ad-hoc: Import the `nixos/cellerd.nix` from [the repository](https://github.com/blitz/celler).
- Flakes: Add `github:blitz/celler` as an input, then import `celler.nixosModules.cellerd`.

## Configuration

> Note: These options are subject to change.

```nix
{
  services.cellerd = {
    enable = true;

    # Replace with absolute path to your environment file
    environmentFile = "/etc/cellerd.env";

    settings = {
      listen = "[::]:8080";

      jwt = {
        rs256-public-key = ./public-key.pem;
      };

      # Data chunking
      #
      # Warning: If you change any of the values here, it will be
      # difficult to reuse existing chunks for newly-uploaded NARs
      # since the cutpoints will be different. As a result, the
      # deduplication ratio will suffer for a while after the change.
      chunking = {
        # The minimum NAR size to trigger chunking
        #
        # If 0, chunking is disabled entirely for newly-uploaded NARs.
        # If 1, all NARs are chunked.
        nar-size-threshold = 64 * 1024; # 64 KiB

        # The preferred minimum size of a chunk, in bytes
        min-size = 16 * 1024; # 16 KiB

        # The preferred average size of a chunk, in bytes
        avg-size = 64 * 1024; # 64 KiB

        # The preferred maximum size of a chunk, in bytes
        max-size = 256 * 1024; # 256 KiB
      };
    };
  };
}
```

After the new configuration is deployed, the Celler Server will be accessible on port 8080.
It's highly recommended to place it behind a reverse proxy like [NGINX](https://nixos.wiki/wiki/Nginx) to provide HTTPS.

## Operations

The NixOS module installs the `cellerd-celleradm` wrapper which runs the `celleradm` command as the `cellerd` user.
Use this command to [generate new tokens](../../reference/celleradm-cli.md#celleradm-make-token) to be distributed to users.
