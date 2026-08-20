//! HTTP API.

mod binary_cache;
mod v1;

use axum::{response::Html, routing::get, Router};

async fn placeholder() -> Html<&'static str> {
    Html(include_str!("placeholder.html"))
}

pub(crate) fn get_router() -> Router {
    Router::new()
        .route("/", get(placeholder))
        .merge(binary_cache::get_router())
        .merge(v1::get_router())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Axum panics on conflicting routes when it builds the router, and
    /// without this test that panic would only show up at server startup.
    #[test]
    fn router_builds() {
        let _ = get_router();
    }
}
