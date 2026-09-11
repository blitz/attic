# Tutorial

Let's spin up Celler! The easiest way to get started is to run the server on a NixOS system.Follow the [deployment guide](admin-guide/deployment.html) to set up your instance.

## Cache Creation

First, let's create a token that can create the `hello` cache and access it. During the deployment, you created a private key that you can use to create tokens. We will need it now.

Run the following command to create a token:

```console
$ celler admin make-token --signing-key private-key.pem --signing-algorithm=rs256 \
  --sub alice --validity '3 months' --pull 'hello' --push 'hello' --configure-cache 'hello' \ --create-cache 'hello'
eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGljZSIsImV4cCI6MTY4MDI5MzQyNSwiaHR0cHM6Ly9qd3QuYXR0aWMucnMvdjEiOnsiY2FjaGVzIjp7ImFsaWNlLSoiOnsiciI6MSwidyI6MSwiY2MiOjF9fX19.MkSnK6yGDWYUVnYiJF3tQgdTlqstfWlbziFWUr-lKUk
```

We can now log in and create a cache:

```console
# Copy and paste the token from the celler admin output above
$ celler login local http://localhost:8080 eyJ...
✍️ Configuring server "local"

$ celler cache create hello
✨ Created cache "hello" on "local"
```

## Pushing

Let's push `celler` itself to the cache:

```console
$ celler push hello $(which celler)
⚙️ Pushing 1 paths to "hello" on "local" (0 already cached, 45 in upstream)...
✅ r5d7217c0rjd5iiz1g2nhvd15frck9x2-celler-0.1.0 (52.89 MiB/s)
```

The interesting thing is that `celler` automatically skipped over store paths cached by `cache.nixos.org`!
This behavior can be configured on a per-cache basis.

Note that Celler performs content-addressed global deduplication, so when you upload the same store path to another cache, the underlying NAR is only stored once.
Each cache is essentially a restricted view of the global cache.

## Pulling

Now, let's pull it back from the cache.
For demonstration purposes, let's use `--store` to make Nix download to another directory because Celler already exists in `/nix/store`:

```console
# Automatically configures ~/.config/nix/nix.conf for you
$ celler use hello
Configuring Nix to use "hello" on "local":
+ Substituter: http://localhost:8080/hello
+ Trusted Public Key: hello:vlsd7ZHIXNnKXEQShVnd7erE8zcuSKrBWRpV6zTibnA=
+ Access Token

$ nix-store --store $PWD/nix-demo -r $(which celler)
[snip]
copying path '/nix/store/r5d7217c0rjd5iiz1g2nhvd15frck9x2-celler-0.1.0' from 'http://localhost:8080/hello'...
warning: you did not specify '--add-root'; the result might be removed by the garbage collector
/nix/store/r5d7217c0rjd5iiz1g2nhvd15frck9x2-celler-0.1.0

$ ls nix-demo/nix/store/r5d7217c0rjd5iiz1g2nhvd15frck9x2-celler-0.1.0/bin/celler
nix-demo/nix/store/r5d7217c0rjd5iiz1g2nhvd15frck9x2-celler-0.1.0/bin/celler
```

Note that to pull into the actual Nix Store, your user must be considered [trusted](https://nixos.org/manual/nix/stable/command-ref/conf-file.html#conf-trusted-users) by the `nix-daemon`.

## Access Control

Celler performs stateless authentication using signed JWT tokens which contain permissions. We've already created a token in the beginning of the tutorial, but let's elaborate a bit on what you can do with them.

Let's create a token that can only access the `hello` cache:

```console
$ celler admin make-token --signing-key private-key.pem --signing-algorithm=rs256 \
  --sub alice --validity '3 months' --pull hello --push hello
eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGljZSIsImV4cCI6MTY4MDI5MzMzOSwiaHR0cHM6Ly9qd3QuYXR0aWMucnMvdjEiOnsiY2FjaGVzIjp7ImhlbGxvIjp7InIiOjEsInciOjF9fX19.XJsaVfjrX5l7p9z76836KXP6Vixn41QJUfxjiK7D-LM
```

> [!NOTE]
> Tokens only have the permissions you explicitly add to them. Tokens that only have push and pull permissions cannot create caches.

Let's say Alice wants to have her own caches. Instead of creating caches for her, we can let her do it herself:

```console
$ celler admin make-token --signing-key private-key.pem --signing-algorithm=rs256 \
  --sub alice --validity '3 months' --pull 'alice-*' --push 'alice-*' --create-cache 'alice-*'
eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGljZSIsImV4cCI6MTY4MDI5MzQyNSwiaHR0cHM6Ly9qd3QuYXR0aWMucnMvdjEiOnsiY2FjaGVzIjp7ImFsaWNlLSoiOnsiciI6MSwidyI6MSwiY2MiOjF9fX19.MkSnK6yGDWYUVnYiJF3tQgdTlqstfWlbziFWUr-lKUk
```

Now Alice can use this token to _create_ any cache beginning with `alice-` and push to them.
Try passing `--dump-claims` to show the JWT claims without encoding the token to see what's going on.

## Going Public

Let's make the cache public. Making it public gives unauthenticated users pull access:

```console
$ celler cache configure hello --public
✅ Configured "hello" on "local"

# Now we can query the cache without being authenticated
$ curl http://localhost:8080/hello/nix-cache-info
WantMassQuery: 1
StoreDir: /nix/store
Priority: 41
```

## Garbage Collection

It's a bad idea to let binary caches grow unbounded.
Let's configure garbage collection on the cache to automatically delete objects that haven't been accessed in a while:

```
$ celler cache configure hello --retention-period '1s'
✅ Configured "hello" on "local"
```

Now the retention period is only one second.
Instead of waiting for the periodic garbage collection to occur (see `server.toml`), let's trigger it manually:

```bash
cellerd --mode garbage-collector-once
```

Now the store path doesn't exist on the cache anymore!

```console
$ nix-store --store $PWD/nix-demo-2 -r $(which celler)
don't know how to build these paths:
  /nix/store/v660wl07i1lcrrgpr1yspn2va5d1xgjr-celler-0.1.0
error: build of '/nix/store/v660wl07i1lcrrgpr1yspn2va5d1xgjr-celler-0.1.0' failed

$ curl http://localhost:8080/hello/v660wl07i1lcrrgpr1yspn2va5d1xgjr.narinfo
{"code":404,"error":"NoSuchObject","message":"The requested object does not exist."}
```

Let's reset it back to the default, which is to not garbage collect (configure it in `server.toml`):

```console
$ celler cache configure hello --reset-retention-period
✅ Configured "hello" on "local"

$ celler cache info hello
               Public: true
           Public Key: hello:vlsd7ZHIXNnKXEQShVnd7erE8zcuSKrBWRpV6zTibnA=
Binary Cache Endpoint: http://localhost:8080/hello
         API Endpoint: http://localhost:8080/
      Store Directory: /nix/store
             Priority: 41
  Upstream Cache Keys: ["cache.nixos.org-1"]
     Retention Period: Global Default
```

Because of Celler's global deduplication, garbage collection actually happens on three levels:

1. **Local Cache**: When an object is garbage collected, only the mapping between the metadata in the local cache and the NAR in the global cache gets deleted. The local cache loses access to the NAR, but the storage isn't freed.
2. **Global NAR Store**: Orphan NARs not referenced by any local cache then become eligible for deletion.
3. **Global Chunk Store**: Finally, orphan chunks not referenced by any NAR become eligible for deletion. This time the storage space is actually freed and subsequent uploads of the same chunk will actually trigger an upload to the storage backend.

## Summary

In just a few commands, we have:

1. Set up a new Celler server and a binary cache
2. Pushed store paths to it
3. Configured Nix to use the new binary cache
4. Generated access tokens that provide restricted access
5. Made the cache public
6. Performed garbage collection

## What's next

> Note: Celler is an early prototype and everything is subject to change! It may be full of holes and APIs may be changed without backward-compatibility. You might even be required to reset the entire database. I would love to have people give it a try, but please keep that in mind ️:)

While it's easy to get started by running `cellerd` in monolithic mode, for production use it's best to run different components of `cellerd` separately with `--mode`:

- `api-server`: Stateless and can be replicated.
- `garbage-collector`: Performs periodic garbage collection. Cannot be replicated.
