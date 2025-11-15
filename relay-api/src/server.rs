use anyhow::Result;
use axum::{
    extract::Extension,
    routing::{get, post},
    Router,
};
use relay_core::RelayContext;
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing;

use crate::handlers;
use crate::websocket;

pub async fn run(ctx: RelayContext) -> Result<()> {
    let api_port = ctx.config.server.api_port;
    let ctx_clone = ctx.clone();
    
    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/ws", get(websocket::websocket_handler))
        .route("/api/v1/notifications", get(handlers::get_notifications))
        .route("/api/v1/notifications/:id/read", post(handlers::mark_notification_read))
        .route("/api/v1/messages", get(handlers::get_messages))
        .route("/api/v1/messages", post(handlers::send_message))
        .route("/api/v1/conversations", get(handlers::get_conversations))
        .route("/api/v1/preferences", get(handlers::get_preferences))
        .route("/api/v1/preferences", post(handlers::update_preferences))
        .route("/api/v1/device-tokens", post(handlers::register_device_token))
        .layer(
            ServiceBuilder::new()
                .layer(Extension(ctx_clone))
                .layer(CorsLayer::permissive()),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], api_port));
    tracing::info!("Starting API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
