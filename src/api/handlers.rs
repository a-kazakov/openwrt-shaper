use crate::engine::Engine;
use crate::model::{BucketSetRequest, QuotaAdjustRequest, SyncRequest, TurboRequest};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

type AppResult = (StatusCode, Json<Value>);

/// GET /api/v1/state
pub async fn handle_state(State(engine): State<Engine>) -> impl IntoResponse {
    Json(serde_json::to_value(engine.snapshot()).unwrap())
}

/// GET /api/v1/config
pub async fn handle_get_config(State(engine): State<Engine>) -> impl IntoResponse {
    Json(engine.config_json())
}

/// PUT /api/v1/config
pub async fn handle_update_config(
    State(engine): State<Engine>,
    body: axum::body::Bytes,
) -> AppResult {
    match engine.update_config(&body) {
        Ok(()) => (StatusCode::OK, Json(engine.config_json())),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        ),
    }
}

/// POST /api/v1/sync
pub async fn handle_sync(
    State(engine): State<Engine>,
    Json(req): Json<SyncRequest>,
) -> AppResult {
    let starlink_bytes = (req.starlink_used_gb * 1_073_741_824.0) as i64;
    let current_bytes = engine.month_used();
    let delta = starlink_bytes - current_bytes;

    if delta > 0 {
        engine.adjust_quota(delta);
        (
            StatusCode::OK,
            Json(json!({
                "adjusted_by": delta,
                "new_total": starlink_bytes,
                "source": req.source,
            })),
        )
    } else if delta < 0 {
        (
            StatusCode::OK,
            Json(json!({
                "note": "Router shows more than Starlink. No adjustment.",
                "router_bytes": current_bytes,
                "starlink_bytes": starlink_bytes,
            })),
        )
    } else {
        (StatusCode::OK, Json(json!({"note": "Already in sync"})))
    }
}

/// POST /api/v1/quota/adjust
pub async fn handle_quota_adjust(
    State(engine): State<Engine>,
    Json(req): Json<QuotaAdjustRequest>,
) -> AppResult {
    if let Some(set) = req.set_bytes {
        engine.set_quota(set);
    } else if let Some(delta) = req.delta_bytes {
        engine.adjust_quota(delta);
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "provide delta_bytes or set_bytes"})),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::to_value(engine.snapshot()).unwrap()),
    )
}

/// POST /api/v1/quota/reset
pub async fn handle_quota_reset(State(engine): State<Engine>) -> impl IntoResponse {
    engine.reset_billing_cycle();
    Json(serde_json::to_value(engine.snapshot()).unwrap())
}

/// POST /api/v1/device/{mac}/turbo
pub async fn handle_device_turbo(
    State(engine): State<Engine>,
    Path(mac): Path<String>,
    Json(req): Json<TurboRequest>,
) -> AppResult {
    let mut duration_min = req.duration_min;
    if duration_min <= 0 {
        duration_min = 15;
    }
    if duration_min > 60 {
        duration_min = 60;
    }

    let mac = mac.to_lowercase();
    let duration = std::time::Duration::from_secs(duration_min as u64 * 60);

    match engine.set_device_turbo(&mac, duration) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::to_value(engine.snapshot()).unwrap()),
        ),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({"error": e}))),
    }
}

/// DELETE /api/v1/device/{mac}/turbo
pub async fn handle_cancel_turbo(
    State(engine): State<Engine>,
    Path(mac): Path<String>,
) -> AppResult {
    let mac = mac.to_lowercase();
    match engine.cancel_device_turbo(&mac) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::to_value(engine.snapshot()).unwrap()),
        ),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({"error": e}))),
    }
}

/// POST /api/v1/device/{mac}/bucket
pub async fn handle_set_bucket(
    State(engine): State<Engine>,
    Path(mac): Path<String>,
    Json(req): Json<BucketSetRequest>,
) -> AppResult {
    let mac = mac.to_lowercase();
    match engine.set_device_bucket(&mac, req.tokens_mb) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::to_value(engine.snapshot()).unwrap()),
        ),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({"error": e}))),
    }
}

/// GET /api/v1/history
pub async fn handle_history() -> impl IntoResponse {
    Json(json!({"samples": []}))
}
