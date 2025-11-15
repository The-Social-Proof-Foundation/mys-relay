use axum::{
    extract::{ws::WebSocketUpgrade, Extension},
    response::Response,
};
use relay_core::{RelayContext, redis::get_connection};
use serde::Deserialize;
use tracing;
use uuid::Uuid;
use futures_util::{SinkExt, StreamExt};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use relay_core::schema::relay_ws_connections;

#[derive(Deserialize)]
pub struct WsQuery {
    user_address: String,
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Extension(ctx): Extension<RelayContext>,
) -> Response {
    // Extract user_address from query string manually
    let user_address = "default".to_string(); // TODO: Extract from query string
    ws.on_upgrade(move |socket| handle_socket(socket, user_address, ctx))
}

async fn handle_socket(
    socket: axum::extract::ws::WebSocket,
    user_address: String,
    ctx: RelayContext,
) {
    tracing::info!("WebSocket connection established for user: {}", user_address);
    
    let (mut sender, mut receiver) = socket.split();
    let connection_id = Uuid::new_v4().to_string();
    
    // Register connection in database
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to get DB connection: {}", e);
            return;
        }
    };
    
    if let Err(e) = diesel::insert_into(relay_ws_connections::table)
        .values((
            relay_ws_connections::user_address.eq(&user_address),
            relay_ws_connections::connection_id.eq(&connection_id),
            relay_ws_connections::connected_at.eq(Utc::now()),
            relay_ws_connections::last_heartbeat_at.eq(Utc::now()),
        ))
        .execute(&mut conn)
        .await
    {
        tracing::error!("Failed to register WebSocket connection: {}", e);
    }
    
    // Clone for tasks
    let ctx_send = ctx.clone();
    let ctx_recv = ctx.clone();
    let user_address_send = user_address.clone();
    let connection_id_recv = connection_id.clone();
    
    // Spawn task to read from Redis stream and forward to WebSocket
    let mut send_task = tokio::spawn(async move {
        let stream_key = format!("STREAM:CHAT:{}", user_address_send);
        let mut last_id = "0".to_string();
        
        loop {
            let mut redis_conn = match get_connection(&ctx_send.redis_pool).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to get Redis connection: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };
            
            // Read from Redis stream
            let result: Result<Vec<(String, Vec<(String, Vec<(String, String)>)>)>, redis::RedisError> = redis::cmd("XREAD")
                .arg("BLOCK")
                .arg(1000) // Block for 1 second
                .arg("STREAMS")
                .arg(&stream_key)
                .arg(&last_id)
                .query_async(&mut redis_conn)
                .await;
            
            match result {
                Ok(streams) => {
                    for (_, messages) in streams {
                        for (msg_id, fields) in messages {
                            last_id = msg_id;
                            
                            // Find the "data" field - fields is Vec<(String, String)>
                            let mut data_value = None;
                            for (i, (key, value)) in fields.iter().enumerate() {
                                if key == "data" && i + 1 < fields.len() {
                                    data_value = Some(&fields[i + 1].1);
                                    break;
                                }
                            }
                            
                            if let Some(data) = data_value {
                                // Send to WebSocket
                                if let Err(e) = sender.send(axum::extract::ws::Message::Text(data.clone())).await {
                                    tracing::error!("Failed to send WebSocket message: {}", e);
                                    return;
                                }
                            }
                        }
                    }
                }
                Err(e) if e.kind() == redis::ErrorKind::TypeError => {
                    // No messages, continue
                    continue;
                }
                Err(e) => {
                    tracing::error!("Redis stream read error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    });
    
    // Handle incoming WebSocket messages (heartbeats, etc.)
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Ping(_)) => {
                    // Update heartbeat
                    let mut conn = match ctx_recv.db_pool.get().await {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    
                    diesel::update(relay_ws_connections::table)
                        .filter(relay_ws_connections::connection_id.eq(&connection_id_recv))
                        .set(relay_ws_connections::last_heartbeat_at.eq(Utc::now()))
                        .execute(&mut conn)
                        .await
                        .ok();
                }
                Ok(axum::extract::ws::Message::Close(_)) => {
                    break;
                }
                _ => {}
            }
        }
        
        // Mark connection as disconnected
        let mut conn = match ctx_recv.db_pool.get().await {
            Ok(c) => c,
            Err(_) => return,
        };
        
        diesel::update(relay_ws_connections::table)
            .filter(relay_ws_connections::connection_id.eq(&connection_id_recv))
            .set(relay_ws_connections::disconnected_at.eq(Utc::now()))
            .execute(&mut conn)
            .await
            .ok();
    });
    
    // Wait for either task to complete
    tokio::select! {
        _ = &mut send_task => {}
        _ = &mut recv_task => {}
    }
    
    tracing::info!("WebSocket connection closed for user: {}", user_address);
}
