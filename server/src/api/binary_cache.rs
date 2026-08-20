//! Nix Binary Cache server.
//!
//! This module implements the Nix Binary Cache API.
//!
//! The implementation is based on the specifications at <https://github.com/fzakaria/nix-http-binary-cache-api-spec>.

use std::collections::VecDeque;
use std::io::Error as IoError;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http;
use axum::{
    body::Body,
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use futures::stream::BoxStream;
use futures::TryStreamExt as _;
use serde::Serialize;
use tokio_util::io::ReaderStream;
use tracing::instrument;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::database::entity::build_trace::{self, Entity as BuildTrace};
use crate::database::entity::chunk::ChunkModel;
use crate::database::AtticDatabase;
use crate::error::{ErrorKind, ServerError, ServerResult};
use crate::narinfo::NarInfo;
use crate::nix_manifest;
use crate::storage::{Download, StorageBackend};
use crate::{RequestState, State};
use attic::api::v1::build_trace::{BuildTraceEntry, BUILD_TRACE_PREFIX, BUILD_TRACE_SUFFIX};
use attic::cache::CacheName;
use attic::io::merge_chunks;
use attic::mime;
use attic::nix_store::StorePathHash;

/// Nix cache information.
///
/// An example of a correct response is as follows:
///
/// ```text
/// StoreDir: /nix/store
/// WantMassQuery: 1
/// Priority: 40
/// ```
#[derive(Debug, Clone, Serialize)]
struct NixCacheInfo {
    /// Whether this binary cache supports bulk queries.
    #[serde(rename = "WantMassQuery")]
    want_mass_query: bool,

    /// The Nix store path this binary cache uses.
    #[serde(rename = "StoreDir")]
    store_dir: PathBuf,

    /// The priority of the binary cache.
    ///
    /// A lower number denotes a higher priority.
    /// <https://cache.nixos.org> has a priority of 40.
    #[serde(rename = "Priority")]
    priority: i32,
}

impl IntoResponse for NixCacheInfo {
    fn into_response(self) -> Response {
        match nix_manifest::to_string(&self) {
            Ok(body) => Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", mime::NIX_CACHE_INFO)
                .body(body)
                .unwrap()
                .into_response(),
            Err(e) => e.into_response(),
        }
    }
}

/// Gets information on a cache.
#[instrument(skip_all, fields(cache_name))]
async fn get_nix_cache_info(
    Extension(state): Extension<State>,
    Extension(req_state): Extension<RequestState>,
    Path(cache_name): Path<CacheName>,
) -> ServerResult<NixCacheInfo> {
    let database = state.database().await?;
    let cache = req_state
        .auth
        .auth_cache(database, &cache_name, |cache, permission| {
            permission.require_pull()?;
            Ok(cache)
        })
        .await?;

    req_state.set_public_cache(cache.is_public);

    let info = NixCacheInfo {
        want_mass_query: true,
        store_dir: cache.store_dir.into(),
        priority: cache.priority,
    };

    Ok(info)
}

/// Gets various information on a store path hash.
///
/// `/:cache/:path`, which may be one of
/// - GET `/:cache/{storePathHash}.narinfo`
/// - HEAD `/:cache/{storePathHash}.narinfo`
/// - GET `/:cache/{storePathHash}.ls` (not implemented)
#[instrument(skip_all, fields(cache_name, path))]
#[axum_macros::debug_handler]
async fn get_store_path_info(
    Extension(state): Extension<State>,
    Extension(req_state): Extension<RequestState>,
    Path((cache_name, path)): Path<(CacheName, String)>,
) -> ServerResult<NarInfo> {
    let components: Vec<&str> = path.splitn(2, '.').collect();

    if components.len() != 2 {
        return Err(ErrorKind::NotFound.into());
    }

    // TODO: Other endpoints
    if components[1] != "narinfo" {
        return Err(ErrorKind::NotFound.into());
    }

    let store_path_hash = StorePathHash::new(components[0].to_string())?;

    tracing::debug!(
        "Received request for {}.narinfo in {:?}",
        store_path_hash.as_str(),
        cache_name
    );

    let (object, cache, nar, _) = state
        .database()
        .await?
        .find_object_and_chunks_by_store_path_hash(&cache_name, &store_path_hash, false)
        .await?;

    let permission = req_state
        .auth
        .get_permission_for_cache(&cache_name, cache.is_public);
    permission.require_pull()?;

    req_state.set_public_cache(cache.is_public);

    let mut narinfo = object.to_nar_info(&nar)?;

    if narinfo.signature().is_none() {
        let keypair = cache.keypair()?;
        narinfo.sign(&keypair);
    }

    Ok(narinfo)
}

/// Gets a NAR.
///
/// - GET `:cache/nar/{storePathHash}.nar`
///
/// Here we use the store path hash not the NAR hash or file hash
/// for better logging. In reality, the files are deduplicated by
/// content-addressing.
#[instrument(skip_all, fields(cache_name, path))]
async fn get_nar(
    Extension(state): Extension<State>,
    Extension(req_state): Extension<RequestState>,
    Path((cache_name, path)): Path<(CacheName, String)>,
) -> ServerResult<Response> {
    let components: Vec<&str> = path.splitn(2, '.').collect();

    if components.len() != 2 {
        return Err(ErrorKind::NotFound.into());
    }

    if components[1] != "nar" {
        return Err(ErrorKind::NotFound.into());
    }

    let store_path_hash = StorePathHash::new(components[0].to_string())?;

    tracing::debug!(
        "Received request for {}.nar in {:?}",
        store_path_hash.as_str(),
        cache_name
    );

    let database = state.database().await?;

    let (object, cache, _nar, chunks) = database
        .find_object_and_chunks_by_store_path_hash(&cache_name, &store_path_hash, true)
        .await?;

    let permission = req_state
        .auth
        .get_permission_for_cache(&cache_name, cache.is_public);
    permission.require_pull()?;

    req_state.set_public_cache(cache.is_public);

    if chunks.iter().any(Option::is_none) {
        // at least one of the chunks is missing :(
        return Err(ErrorKind::IncompleteNar.into());
    }

    database.bump_object_last_accessed(object.id).await?;

    if chunks.len() == 1 {
        // single chunk
        let chunk = chunks[0].as_ref().unwrap();
        let remote_file = &chunk.remote_file.0;
        let storage = state.storage().await?;
        match storage.download_file_db(remote_file, false).await? {
            Download::Url(url) => Ok(Redirect::temporary(&url).into_response()),
            Download::AsyncRead(stream) => {
                let stream = ReaderStream::new(stream).map_err(|e| {
                    tracing::error!(%e, "Failed to download single-chunk file");
                    e
                });
                let body = Body::from_stream(stream);

                Ok((
                    [(
                        http::header::CONTENT_TYPE,
                        http::HeaderValue::from_static(mime::NAR),
                    )],
                    body,
                )
                    .into_response())
            }
        }
    } else {
        // reassemble NAR
        fn io_error<E: std::error::Error + Send + Sync + 'static>(e: E) -> IoError {
            IoError::other(e)
        }

        let streamer = |chunk: ChunkModel, storage: Arc<Box<dyn StorageBackend + 'static>>| async move {
            match storage
                .download_file_db(&chunk.remote_file.0, true)
                .await
                .map_err(|e| {
                    tracing::error!(%e, "Failed to download chunk");
                    io_error(e)
                })? {
                Download::Url(_) => Err(IoError::other("URLs not supported for NAR reassembly")),
                Download::AsyncRead(stream) => {
                    let stream: BoxStream<_> = Box::pin(ReaderStream::new(stream));
                    Ok(stream)
                }
            }
        };

        let chunks: VecDeque<_> = chunks.into_iter().map(Option::unwrap).collect();
        let storage = state.storage().await?.clone();

        // TODO: Make num_prefetch configurable
        // The ideal size depends on the average chunk size
        let path_for_error = path.clone();
        let merged = merge_chunks(chunks, streamer, storage, 2).map_err(move |e| {
            tracing::error!(%e, path = %path_for_error, "Merging chunks failed");
            e
        });
        let body = Body::from_stream(merged);

        Ok((
            [(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static(mime::NAR),
            )],
            body,
        )
            .into_response())
    }
}

/// Resolves a derivation output to its realized store path.
///
/// - GET `/:cache/build-trace-v2/{drvName}.drv/{outputName}.doi`
#[instrument(skip_all, fields(cache_name, drv_path, output))]
async fn get_build_trace(
    Extension(state): Extension<State>,
    Extension(req_state): Extension<RequestState>,
    Path((cache_name, drv_path, output)): Path<(CacheName, String, String)>,
) -> ServerResult<Json<BuildTraceEntry>> {
    let output_name = output
        .strip_suffix(BUILD_TRACE_SUFFIX)
        .ok_or_else(|| ServerError::from(ErrorKind::NotFound))?;

    let database = state.database().await?;
    let cache = req_state
        .auth
        .auth_cache(database, &cache_name, |cache, permission| {
            permission.require_pull()?;
            Ok(cache)
        })
        .await?;

    req_state.set_public_cache(cache.is_public);

    tracing::debug!(
        "Received build trace request for {}!{} in {:?}",
        drv_path,
        output_name,
        cache_name
    );

    let entry = BuildTrace::find()
        .filter(build_trace::Column::CacheId.eq(cache.id))
        .filter(build_trace::Column::DrvPath.eq(drv_path))
        .filter(build_trace::Column::OutputName.eq(output_name))
        .one(database)
        .await
        .map_err(ServerError::database_error)?
        .ok_or_else(|| ServerError::from(ErrorKind::NoSuchObject))?;

    // This handler repeats the object lookup the upload already did, because a
    // trace outlives the object a GC reaps. Serving such a trace makes this
    // cache assert a store path it holds no NAR for, and the client may then
    // fetch that path from some other cache.
    let out_path_hash = StorePathHash::new(entry.out_path_hash)
        .map_err(|_| ServerError::from(ErrorKind::NoSuchObject))?;
    let object = database
        .find_object_by_store_path_hash(cache.id, &out_path_hash)
        .await?;

    // The name has to match as well, because an object upsert can change
    // `store_path` under a fixed hash, which leaves the trace naming a path
    // this cache no longer claims.
    if object.store_path_base_name() != Some(entry.out_path.as_str()) {
        return Err(ErrorKind::NoSuchObject.into());
    }

    Ok(Json(BuildTraceEntry {
        out_path: entry.out_path,
        signatures: entry.signatures.0,
    }))
}

pub fn get_router() -> Router {
    Router::new()
        .route("/{cache}/nix-cache-info", get(get_nix_cache_info))
        .route("/{cache}/{path}", get(get_store_path_info))
        .route("/{cache}/nar/{path}", get(get_nar))
        .route(
            &format!("/{{cache}}/{BUILD_TRACE_PREFIX}/{{drv_path}}/{{output}}"),
            get(get_build_trace),
        )
}
