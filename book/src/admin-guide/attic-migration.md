# Migration from Attic

This guide explains how to migrate from Attic to Celler.

## Migration Guide

> [!CAUTION]
> The migration process is not regularly tested. Use at your own risk and please backup your database before proceeding.

> [!NOTE]
> The Celler server's API is deliberately not compatible with the Attic CLI tool. Use the Celler CLI instead.

### Step 0: Backup Your Database

Before proceeding, make sure to backup your database.

### Step 1: Adapt Server Configuration

Adapt your NixOS configuration to use Celler instead of Attic:

- Point your Flake input to `github:blitz/celler`.
- Rename `services.atticd` to `services.cellerd`.

JWT secrets are not configured via environment variables in Celler. Existing BASE64-encoded RS256 private keys have to be decoded via `base64 -d`. See [the deployment guide](deployment/nixos.md) for how to convert it to a public key and add it to the configuration.

Similarly, HS256 keys can be decoded via `base64 -d` and passed to `services.cellerd.settings.jwt.hs256-secret-key` in the server config.

### Step 2: Adapt Client Configuration

Client configuration needs to be adapted as well.

- Point your Flake input to `github:blitz/celler`.
- Replace the `attic` package with `celler` in your client configuration.
