# Chunking

Celler can optionally use the [FastCDC
algorithm](https://www.usenix.org/conference/atc16/technical-sessions/presentation/xia)
to split uploaded NARs into chunks for deduplication. For typical cache content,
this significantly reduces the amount of storage required in the underlying
block storage.

## Considerations For Enabling Chunking

Without chunking, Celler can typically serve pre-signed URLs to the store paths
on the underlying block storage. This shifts the burden of downloading store
paths to the requesting client.

With chunking enabled, the Celler daemon has to download individual chunks and
assemble them itself. This substantially increases the work the daemon has to
perform itself.

In addition, chunking, especially with small chunks, will cause a high rate of
API calls to the underlying block storage from the Celler daemon. Depending on
your block storage provider, this may cause Celler's requests to be rate
limited. When this happens, downloading store paths becomes either very slow or
will fail entirely.

Celler's behavior under rate limiting is still not ideal and will improve when
the respective code paths have been made more robust. But even with that in
mind, the rate limiting issue and the increased strain on the Celler daemon may
outweigh the storage cost savings.

## Configuration

There are four main parameters that control chunking in Celler:

- `nar-size-threshold`: The minimum NAR size to trigger chunking
    - When set to 0, chunking is disabled entirely for newly-uploaded NARs
    - When set to 1, all newly-uploaded NARs are chunked
- `min-size`: The preferred minimum size of a chunk, in bytes
- `avg-size`: The preferred average size of a chunk, in bytes
- `max-size`: The preferred maximum size of a chunk, in bytes

These parameters are included in the `[chunking]` section in the configuration:

```toml
# Data chunking
#
# Warning: If you change any of the values here, it will be
# difficult to reuse existing chunks for newly-uploaded NARs
# since the cutpoints will be different. As a result, the
# deduplication ratio will suffer for a while after the change.
[chunking]
# The minimum NAR size to trigger chunking
#
# If 0, chunking is disabled entirely for newly-uploaded NARs.
# If 1, all newly-uploaded NARs are chunked.
nar-size-threshold = 131072 # chunk files that are 128 KiB or larger

# The preferred minimum size of a chunk, in bytes
min-size = 65536            # 64 KiB

# The preferred average size of a chunk, in bytes
avg-size = 131072           # 128 KiB

# The preferred maximum size of a chunk, in bytes
max-size = 262144           # 256 KiB
```
