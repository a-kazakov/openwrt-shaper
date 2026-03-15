use crate::engine::Engine;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tracing::{info, warn};

/// GET /ws — upgrade to WebSocket
pub async fn handle_ws(
    State(engine): State<Engine>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, engine))
}

async fn handle_socket(socket: WebSocket, engine: Engine) {
    let (mut sender, mut receiver) = socket.split();

    // Send initial state
    let snapshot = engine.snapshot();
    if let Ok(data) = serde_json::to_string(&snapshot) {
        if sender.send(Message::Text(data.into())).await.is_err() {
            return;
        }
    }

    // Subscribe to engine snapshot updates
    let mut rx = engine.subscribe();

    // Spawn task to forward snapshots to this client
    let mut send_task = tokio::spawn(async move {
        loop {
            if rx.changed().await.is_err() {
                break;
            }
            let snapshot = rx.borrow().clone();
            if let Some(snap) = snapshot {
                match serde_json::to_string(&snap) {
                    Ok(data) => {
                        if sender.send(Message::Text(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("ws: serialize snapshot: {e}");
                    }
                }
            }
        }
    });

    // Read loop — handle client disconnect
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }

    info!("ws: client disconnected");
}
