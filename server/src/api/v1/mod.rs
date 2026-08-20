mod build_trace;
mod cache_config;
mod get_missing_paths;
mod upload_path;

use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};

use attic::api::v1::build_trace::BUILD_TRACE_PREFIX;

pub(crate) fn get_router() -> Router {
    Router::new()
        .route(
            "/_api/v1/get-missing-paths",
            post(get_missing_paths::get_missing_paths),
        )
        .route("/_api/v1/upload-path", put(upload_path::upload_path))
        .route(
            &format!("/{{cache}}/{BUILD_TRACE_PREFIX}/{{drv_path}}/{{output}}"),
            put(build_trace::put_build_trace),
        )
        .route(
            "/{cache}/attic-cache-info",
            get(cache_config::get_cache_config),
        )
        .route(
            "/_api/v1/cache-config/{cache}",
            get(cache_config::get_cache_config),
        )
        .route(
            "/_api/v1/cache-config/{cache}",
            post(cache_config::create_cache),
        )
        .route(
            "/_api/v1/cache-config/{cache}",
            patch(cache_config::configure_cache),
        )
        .route(
            "/_api/v1/cache-config/{cache}",
            delete(cache_config::destroy_cache),
        )
}
