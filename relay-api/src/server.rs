use anyhow::Result;
use axum::{
    extract::Extension,
    middleware,
    routing::{get, post},
    Router,
};
use relay_core::RelayContext;
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::cors::{CorsLayer, Any};
use tracing;
use std::env;

use crate::handlers;
use crate::websocket;
use crate::auth;

pub async fn run(ctx: RelayContext) -> Result<()> {
    let api_port = ctx.config.server.api_port;
    let ctx_clone = ctx.clone();
    
    // Configure CORS - allow specific origins or all if CORS_ORIGINS not set
    let cors_layer = if let Ok(origins) = env::var("CORS_ORIGINS") {
        // Parse comma-separated origins
        let origin_list: Vec<&str> = origins.split(',').map(|s| s.trim()).collect();
        let mut cors = CorsLayer::new();
        for origin in origin_list {
            if let Ok(parsed) = origin.parse::<axum::http::HeaderValue>() {
                cors = cors.allow_origin(parsed);
            }
        }
        cors.allow_methods(Any)
            .allow_headers(Any)
            .allow_credentials(true)
    } else {
        // Default to permissive for development, but log warning
        tracing::warn!("CORS_ORIGINS not set, using permissive CORS. Set CORS_ORIGINS for production!");
        CorsLayer::permissive()
    };
    
    let app = Router::new()
            .route("/health", get(handlers::health))
            .route("/ws", get(websocket::websocket_handler))
            .route("/api/v1/auth/token", post(handlers::generate_token))
            .route("/api/v1/notifications", get(handlers::get_notifications))
            .route("/api/v1/notifications/counts", get(handlers::get_notification_counts))
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
                    .layer(middleware::from_fn(auth::auth_middleware))
                    .layer(cors_layer),
            );

    let addr = SocketAddr::from(([0, 0, 0, 0], api_port));
    tracing::info!("Starting API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
