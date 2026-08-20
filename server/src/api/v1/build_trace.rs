//! Build trace upload.

use anyhow::anyhow;
use axum::extract::{Extension, Json, Path};
use chrono::Utc;
use sea_orm::sea_query::{Expr, OnConflict, Query};
use sea_orm::{ColumnTrait, ConnectionTrait};
use tracing::instrument;

use crate::database::entity::build_trace::{self, Entity as BuildTrace};
use crate::database::entity::object;
use crate::database::entity::Json as DbJson;
use crate::database::AtticDatabase;
use crate::error::{ErrorKind, ServerError, ServerResult};
use crate::{RequestState, State};
use attic::api::v1::build_trace::{BuildTraceEntry, BUILD_TRACE_SUFFIX};
use attic::cache::CacheName;
use attic::nix_store::StorePath;

/// Must match the column widths in the `build_trace` migration.
const MAX_DRV_PATH_LEN: usize = 255;
const MAX_OUTPUT_NAME_LEN: usize = 128;
const MAX_OUT_PATH_LEN: usize = 255;

/// Records the store path that a derivation output realized to.
///
/// - PUT `/:cache/build-trace-v2/{drvName}.drv/{outputName}.doi`
#[instrument(skip_all, fields(cache_name, drv_path, output))]
pub(crate) async fn put_build_trace(
    Extension(state): Extension<State>,
    Extension(req_state): Extension<RequestState>,
    Path((cache_name, drv_path, output)): Path<(CacheName, String, String)>,
    Json(payload): Json<BuildTraceEntry>,
) -> ServerResult<()> {
    let output_name = output
        .strip_suffix(BUILD_TRACE_SUFFIX)
        .ok_or_else(|| ServerError::from(ErrorKind::NotFound))?;

    let database = state.database().await?;
    let cache = req_state
        .auth
        .auth_cache(database, &cache_name, |cache, permission| {
            permission.require_push()?;
            Ok(cache)
        })
        .await?;

    tracing::debug!(
        "Storing build trace for {}!{} in {:?}",
        drv_path,
        output_name,
        cache_name
    );

    // SQLite ignores varchar lengths, and PostgreSQL would answer 500 instead.
    if drv_path.len() > MAX_DRV_PATH_LEN
        || output_name.len() > MAX_OUTPUT_NAME_LEN
        || payload.out_path.len() > MAX_OUT_PATH_LEN
    {
        return Err(ErrorKind::RequestError(anyhow!("Build trace entry too long")).into());
    }

    // Nix turns an unparseable path into a build failure.
    let out_path =
        StorePath::from_base_name(&payload.out_path).map_err(ServerError::request_error)?;
    let out_path_hash = out_path.to_hash();

    // The object lookup keeps the binding inside the tenant, because a cache
    // holding no objects could otherwise assert a path that a client then
    // fetches from some other cache.
    let object = database
        .find_object_by_store_path_hash(cache.id, &out_path_hash)
        .await?;

    if object.store_path_base_name() != Some(payload.out_path.as_str()) {
        return Err(ErrorKind::RequestError(anyhow!(
            "outPath does not match the store path recorded for that hash"
        ))
        .into());
    }

    // The insert selects its row from `object`, so a GC that reaps the object
    // between the lookup and the write leaves no row to insert. That repeats
    // the check above at the moment of the write.
    //
    // These expressions must stay in lockstep with the column list below.
    let source = Query::select()
        .expr(Expr::val(cache.id))
        .expr(Expr::val(drv_path))
        .expr(Expr::val(output_name.to_owned()))
        .expr(Expr::val(payload.out_path))
        .expr(Expr::val(out_path_hash.as_str().to_owned()))
        .expr(Expr::val(DbJson(payload.signatures)))
        .expr(Expr::val(Utc::now()))
        .expr(Expr::val(req_state.auth.username().map(str::to_string)))
        .from(object::Entity)
        .and_where(object::Column::CacheId.eq(cache.id))
        .and_where(object::Column::StorePathHash.eq(out_path_hash.as_str()))
        .to_owned();

    let mut insert = Query::insert();
    insert.into_table(BuildTrace).columns([
        build_trace::Column::CacheId,
        build_trace::Column::DrvPath,
        build_trace::Column::OutputName,
        build_trace::Column::OutPath,
        build_trace::Column::OutPathHash,
        build_trace::Column::Signatures,
        build_trace::Column::CreatedAt,
        build_trace::Column::CreatedBy,
    ]);
    insert
        .select_from(source)
        // `select_from` only fails when the column count differs from the
        // number of expressions above.
        .map_err(ServerError::database_error)?
        .on_conflict(
            OnConflict::columns([
                build_trace::Column::CacheId,
                build_trace::Column::DrvPath,
                build_trace::Column::OutputName,
            ])
            // `CreatedAt` stays out of the update, so the column keeps the
            // creation time that age-based cleanup reads.
            .update_columns([
                build_trace::Column::OutPath,
                build_trace::Column::OutPathHash,
                build_trace::Column::Signatures,
                build_trace::Column::CreatedBy,
            ])
            .to_owned(),
        );

    let insertion = database
        .execute(&insert)
        .await
        .map_err(ServerError::database_error)?;

    if insertion.rows_affected() == 0 {
        // A GC reaped the object between the lookup and the insert.
        return Err(ErrorKind::NoSuchObject.into());
    }

    Ok(())
}
