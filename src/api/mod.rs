pub mod handlers;
pub mod websocket;

use crate::engine::Engine;
use axum::routing::{delete, get, post, put};
use axum::Router;

/// Build the axum router with all REST endpoints and WebSocket.
pub fn router(engine: Engine) -> Router {
    Router::new()
        .route("/api/v1/state", get(handlers::handle_state))
        .route("/api/v1/config", get(handlers::handle_get_config))
        .route("/api/v1/config", put(handlers::handle_update_config))
        .route("/api/v1/sync", post(handlers::handle_sync))
        .route("/api/v1/quota/adjust", post(handlers::handle_quota_adjust))
        .route("/api/v1/quota/reset", post(handlers::handle_quota_reset))
        .route("/api/v1/history", get(handlers::handle_history))
        .route("/api/v1/interfaces", get(handlers::handle_list_interfaces))
        .route(
            "/api/v1/device/{mac}/turbo",
            post(handlers::handle_device_turbo),
        )
        .route(
            "/api/v1/device/{mac}/turbo",
            delete(handlers::handle_cancel_turbo),
        )
        .route(
            "/api/v1/device/{mac}/bucket",
            post(handlers::handle_set_bucket),
        )
        .route(
            "/api/v1/device/{mac}/mode",
            post(handlers::handle_set_device_mode),
        )
        .route("/ws", get(websocket::handle_ws))
        .fallback(crate::web::static_handler)
        .with_state(engine)
}
