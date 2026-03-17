use crate::engine::Engine;
use crate::model::{BucketSetRequest, QuotaAdjustRequest, SyncRequest, TurboRequest};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

type AppResult = (StatusCode, Json<Value>);

fn snapshot_json(engine: &Engine) -> AppResult {
    match serde_json::to_value(engine.snapshot()) {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("serialize: {e}")})),
        ),
    }
}

pub async fn handle_state(State(engine): State<Engine>) -> AppResult {
    snapshot_json(&engine)
}

pub async fn handle_get_config(State(engine): State<Engine>) -> impl IntoResponse {
    Json(engine.config_json())
}

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

pub async fn handle_sync(
    State(engine): State<Engine>,
    Json(req): Json<SyncRequest>,
) -> AppResult {
    if !req.starlink_used_gb.is_finite() || req.starlink_used_gb < 0.0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "starlink_used_gb must be a non-negative finite number"})),
        );
    }

    // Starlink uses base-10 GB (1 GB = 1,000,000,000 bytes)
    let starlink_bytes = (req.starlink_used_gb * 1_000_000_000.0) as i64;
    let current_bytes = engine.month_used();
    let delta = starlink_bytes - current_bytes;

    // Record the absolute gap for mismatch warnings
    engine.set_sync_gap((starlink_bytes - current_bytes).unsigned_abs() as i64);

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
        // Perfect sync — clear the gap
        engine.set_sync_gap(0);
        (StatusCode::OK, Json(json!({"note": "Already in sync"})))
    }
}

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

    snapshot_json(&engine)
}

pub async fn handle_quota_reset(State(engine): State<Engine>) -> AppResult {
    engine.reset_billing_cycle();
    snapshot_json(&engine)
}

pub async fn handle_device_turbo(
    State(engine): State<Engine>,
    Path(mac): Path<String>,
    Json(req): Json<TurboRequest>,
) -> AppResult {
    let mut duration_min = req.duration_min;
    if duration_min <= 0 {
        duration_min = 15;
    }
    if duration_min > 360 {
        duration_min = 360;
    }

    let mac = mac.to_lowercase();
    let duration = std::time::Duration::from_secs(duration_min as u64 * 60);

    match engine.set_device_turbo(&mac, duration) {
        Ok(()) => snapshot_json(&engine),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({"error": e}))),
    }
}

pub async fn handle_cancel_turbo(
    State(engine): State<Engine>,
    Path(mac): Path<String>,
) -> AppResult {
    let mac = mac.to_lowercase();
    match engine.cancel_device_turbo(&mac) {
        Ok(()) => snapshot_json(&engine),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({"error": e}))),
    }
}

pub async fn handle_set_bucket(
    State(engine): State<Engine>,
    Path(mac): Path<String>,
    Json(req): Json<BucketSetRequest>,
) -> AppResult {
    let mac = mac.to_lowercase();
    match engine.set_device_bucket(&mac, req.tokens_mb) {
        Ok(()) => snapshot_json(&engine),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({"error": e}))),
    }
}

pub async fn handle_history() -> impl IntoResponse {
    Json(json!({"samples": []}))
}

pub async fn handle_list_interfaces() -> impl IntoResponse {
    let ifaces = crate::netctl::devices::list_interfaces();
    match serde_json::to_value(&ifaces) {
        Ok(v) => Json(v),
        Err(e) => Json(json!({"error": format!("serialize: {e}")})),
    }
}
