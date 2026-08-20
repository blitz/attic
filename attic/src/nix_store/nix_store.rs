//! High-level Nix Store interface.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::Arc;

use bytes::Bytes;
use futures::{stream, Stream, StreamExt};
use nix_daemon::{Progress, Store};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;

use super::{to_base_name, FloatingOutput, StorePath, ValidPathInfo};
use crate::error::AtticResult;
use crate::hash::Hash;
use crate::AtticError;

/// High-level wrapper for the Unix Domain Socket Nix Store.
pub struct NixStore {
    daemon: Arc<Mutex<nix_daemon::nix::DaemonStore<UnixStream>>>,

    /// Path to the Nix store itself.
    store_dir: PathBuf,
}

const DAEMON_SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";

async fn daemon_connect() -> AtticResult<nix_daemon::nix::DaemonStore<UnixStream>> {
    let daemon = nix_daemon::nix::DaemonStore::builder()
        .connect_unix(DAEMON_SOCKET_PATH)
        .await
        .map_err(|e| AtticError::StoreConnectError {
            reason: e.to_string(),
        })?;
    Ok(daemon)
}

impl NixStore {
    pub async fn connect() -> AtticResult<Self> {
        Ok(Self {
            daemon: Arc::new(Mutex::new(daemon_connect().await?)),
            // TODO: Make this method async and call nix-instantiate --raw --eval -E 'builtins.storeDir'
            store_dir: PathBuf::from_str("/nix/store").unwrap(),
        })
    }

    /// Returns the Nix store directory.
    pub fn store_dir(&self) -> &Path {
        &self.store_dir
    }

    /// Returns the base store path of a path, following any symlinks.
    ///
    /// This is a simple wrapper over `parse_store_path` that also
    /// follows symlinks.
    pub fn follow_store_path<P: AsRef<Path>>(&self, path: P) -> AtticResult<StorePath> {
        // Some cases to consider:
        //
        // - `/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-nixos-system-x/sw` (a symlink to sw)
        //    - `eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-nixos-system-x`
        //    - We don't resolve the `sw` symlink since the full store path is specified
        //      (this is a design decision)
        // - `/run/current-system` (a symlink to profile)
        //    - `eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-nixos-system-x`
        // - `/run/current-system/` (with a trailing slash)
        //    - `eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-nixos-system-x`
        // - `/run/current-system/sw` (a symlink to sw)
        //    - `eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-system-path` (!)
        let path = path.as_ref();
        if path.strip_prefix(&self.store_dir).is_ok() {
            // Is in the store - directly strip regardless of being a symlink or not
            self.parse_store_path(path)
        } else {
            // Canonicalize then parse
            let canon = path.canonicalize()?;
            self.parse_store_path(canon)
        }
    }

    /// Returns the base store path of a path.
    ///
    /// This function does not validate whether the path is actually in the
    /// Nix store or not.
    ///
    /// The path must be under the store directory. See `follow_store_path`
    /// for an alternative that follows symlinks.
    pub fn parse_store_path(&self, path: impl AsRef<Path>) -> AtticResult<StorePath> {
        let base_name = to_base_name(&self.store_dir, path.as_ref())?;
        StorePath::from_base_name(base_name)
    }

    /// Returns the full path for a base store path.
    pub fn get_full_path(&self, store_path: impl AsRef<StorePath>) -> PathBuf {
        self.store_dir.join(&store_path.as_ref().base_name)
    }

    /// Creates a NAR archive from a path.
    ///
    /// This is akin to `nix-store --dump`.
    pub fn nar_from_path(
        &self,
        store_path: impl AsRef<StorePath>,
    ) -> impl Stream<Item = AtticResult<Bytes>> + Unpin + Send {
        let full_store_path = self.get_full_path(store_path);
        let full_store_path_str = full_store_path
            .to_str()
            // TODO Move UTF-8 check to StorePath creation.
            .unwrap()
            .to_owned();

        // We create a new store connection, because the
        // implementation of daemon.nar_from_path is fragile.
        //
        // We also want to stream multiple NARs at the same time,
        // which wouldn't work if we keep holding the daemon
        // connection mutex.
        let setup_fn = async move {
            let daemon = daemon_connect().await?;

            daemon.into_nar_from_path(full_store_path_str).result().await.map_err(|e| AtticError::NarFromPathError { reason: e.to_string() })
        };

        Box::pin(
            stream::once(setup_fn)
                .then(async |s| match s {
                    Ok(reader) => ReaderStream::new(reader)
                        .map(|i| i.map_err(|e| AtticError::NarFromPathError { reason: e.to_string() }))
                        .left_stream(),
                    Err(e) => stream::once(async move { Err(e) }).right_stream(),
                })
                .flatten(),
        )
    }

    /// Returns the closure of a valid path.
    ///
    /// If `flip_directions` is true, the set of paths that can reach `store_path` is
    /// returned.
    pub async fn compute_fs_closure(
        &self,
        store_path: StorePath,
        include_outputs: bool,
    ) -> AtticResult<Vec<StorePath>> {
        self.compute_fs_closure_multi(vec![store_path], include_outputs)
            .await
    }

    /// Returns the closure of a set of valid paths.
    ///
    /// This is the multi-path variant of `compute_fs_closure`.
    pub async fn compute_fs_closure_multi(
        &self,
        store_paths: Vec<StorePath>,
        include_outputs: bool,
    ) -> AtticResult<Vec<StorePath>> {
        let mut unqueried_paths: Vec<StorePath> = store_paths;
        let mut result: BTreeSet<StorePath> = Default::default();

        if include_outputs {
            todo!("include_outputs is not implemented yet.")
        }

        while let Some(unqueried_path) = unqueried_paths.pop() {
            if !result.contains(&unqueried_path) {
                // TODO It would be very cool to keep topological
                // order, so we can push paths to the store in a sane
                // order.
                result.insert(unqueried_path.clone());

                let path_info = self.query_path_info(&unqueried_path).await?.ok_or(
                    AtticError::InvalidStorePath {
                        path: unqueried_path.base_name,
                        reason: "Missing reference",
                    },
                )?;

                unqueried_paths.extend_from_slice(&path_info.references);
            }
        }

        Ok(result.into_iter().collect())
    }

    /// Check whether a given store path is actually valid.
    ///
    /// This returns true, iff `query_path_info` would also have given
    /// you a positive result.
    pub async fn is_valid_path(&self, store_path: impl AsRef<StorePath>) -> AtticResult<bool> {
        let mut daemon = self.daemon.lock().await;

        Ok(daemon
            .is_valid_path(
                self.get_full_path(&store_path)
                    .as_os_str()
                    .to_str()
                    .ok_or_else(|| AtticError::InvalidStorePath {
                        path: store_path.as_ref().base_name.clone(),
                        reason: "Invalid UTF-8",
                    })?,
            )
            .result()
            .await
            .inspect_err(|e| {
                eprintln!(
                    "Failed to query path, considering non-valid: {} {}",
                    self.get_full_path(&store_path).display(),
                    e
                );
            })
            .unwrap_or(false))
    }

    /// Identifies the derivation output a path realized to, if it was floating.
    ///
    /// Returns `None` when the path is not floating, or when its deriver is gone
    /// and the question cannot be answered.
    pub async fn floating_output(
        &self,
        path: impl AsRef<StorePath>,
        deriver: &str,
    ) -> AtticResult<Option<FloatingOutput>> {
        let path = path.as_ref();

        // Only a floating output is absent from the derivation output table, so
        // a deriver recorded there means the path was known ahead of the build.
        //
        // An empty result also covers a path whose deriver is gone, which the
        // validity check below separates out. Keep the two checks in this
        // order.
        if !self.query_valid_derivers(path).await?.is_empty() {
            return Ok(None);
        }

        // The deriver of a substituted path is generally not in the store,
        // since `nix copy` does not transfer `.drv` files, and a built path can
        // outlive its deriver.
        let deriver_path = self.parse_store_path(deriver)?;
        if !self.is_valid_path(&deriver_path).await? {
            return Ok(None);
        }

        let drv_name = deriver_path
            .as_os_str()
            .to_str()
            .ok_or_else(|| AtticError::InvalidStorePath {
                path: deriver_path.base_name.clone(),
                reason: "Invalid UTF-8",
            })?
            .to_owned();

        let outputs = self.query_derivation_output_map(deriver).await?;
        let output_name = outputs
            .into_iter()
            .find(|(_, out)| self.parse_store_path(out).ok().as_ref() == Some(path))
            .map(|(output_name, _)| output_name)
            .ok_or_else(|| AtticError::InvalidStorePath {
                path: path.base_name.clone(),
                reason: "Deriver has no output matching this path",
            })?;

        Ok(Some(FloatingOutput {
            drv_name,
            output_name,
        }))
    }

    /// Returns the derivations in the store known to produce a path.
    ///
    /// Returns nothing for a floating content-addressed output.
    pub async fn query_valid_derivers(
        &self,
        store_path: impl AsRef<StorePath>,
    ) -> AtticResult<Vec<String>> {
        let full_store_path = self.get_full_path(&store_path);
        let full_store_path_str =
            full_store_path
                .to_str()
                .ok_or_else(|| AtticError::InvalidStorePath {
                    path: full_store_path.clone(),
                    reason: "Invalid UTF-8",
                })?;

        let mut daemon = self.daemon.lock().await;
        daemon
            .query_valid_derivers(full_store_path_str)
            .result()
            .await
            .map_err(|e| AtticError::DaemonQueryError {
                op: "QueryValidDerivers",
                reason: e.to_string(),
            })
    }

    /// Maps each output of a derivation to the store path it realized to.
    ///
    /// For a content-addressed derivation the daemon answers from the local
    /// build trace, so this only returns outputs that have been built on this
    /// machine.
    pub async fn query_derivation_output_map(
        &self,
        drv_path: &str,
    ) -> AtticResult<HashMap<String, String>> {
        let mut daemon = self.daemon.lock().await;
        daemon
            .query_derivation_output_map(drv_path)
            .result()
            .await
            .map_err(|e| AtticError::DaemonQueryError {
                op: "QueryDerivationOutputMap",
                reason: e.to_string(),
            })
    }

    /// Returns detailed information on a path.
    pub async fn query_path_info(
        &self,
        store_path: impl AsRef<StorePath>,
    ) -> AtticResult<Option<ValidPathInfo>> {
        let opt_path_info = {
            let full_store_path = self.get_full_path(&store_path);
            let full_store_path_str =
                full_store_path
                    .to_str()
                    .ok_or_else(|| AtticError::InvalidStorePath {
                        path: full_store_path.clone(),
                        reason: "Invalid UTF-8",
                    })?;

            let mut daemon = self.daemon.lock().await;
            daemon
                .query_pathinfo(full_store_path_str)
                .result()
                .await
                .map_err(|_e| AtticError::InvalidStorePath {
                    path: full_store_path.clone(),
                    reason: "Failed to query",
                })?
        };

        opt_path_info
            .map(|path_info| -> AtticResult<_> {
                Ok(ValidPathInfo {
                    path: store_path.as_ref().to_owned(),
                    // TODO The documentation of PathInfo lies that the string has a sha256- prefix.
                    nar_hash: Hash::from_typed(&format!("sha256:{}", path_info.nar_hash))?,
                    nar_size: path_info.nar_size,
                    references: path_info
                        .references
                        .into_iter()
                        .map(|p| -> AtticResult<StorePath> { self.parse_store_path(p) })
                        .collect::<AtticResult<Vec<_>>>()?,
                    sigs: path_info.signatures,
                    ca: path_info.ca,
                    deriver: path_info.deriver,
                })
            })
            .transpose()
    }
}
