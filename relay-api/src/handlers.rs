use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::Json,
};
use relay_core::{RelayContext, redis::get_connection, schema::relay_notifications};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::Utc;

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "relay-api"
    }))
}

#[derive(Deserialize)]
pub struct NotificationQuery {
    pub user_address: String,
    #[serde(default)]
    pub platform_id: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

pub async fn get_notifications(
    Extension(ctx): Extension<RelayContext>,
    Query(params): Query<NotificationQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(100);
    let offset = params.offset.unwrap_or(0);
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut query = relay_notifications::table
        .filter(relay_notifications::user_address.eq(&params.user_address))
        .order(relay_notifications::created_at.desc())
        .limit(limit)
        .offset(offset)
        .into_boxed();

    // Filter by platform_id if provided
    if let Some(platform_id) = &params.platform_id {
        query = query.filter(relay_notifications::platform_id.eq(platform_id));
    }

    let notifications: Vec<(i64, String, String, String, String, Option<serde_json::Value>, Option<String>, Option<chrono::DateTime<chrono::Utc>>, chrono::DateTime<chrono::Utc>)> = match query
        .select((
            relay_notifications::id,
            relay_notifications::user_address,
            relay_notifications::notification_type,
            relay_notifications::title,
            relay_notifications::body,
            relay_notifications::data,
            relay_notifications::platform_id,
            relay_notifications::read_at,
            relay_notifications::created_at,
        ))
        .load(&mut conn)
        .await
    {
        Ok(n) => n,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let result: Vec<serde_json::Value> = notifications
        .into_iter()
        .map(|(id, user_address, notification_type, title, body, data, platform_id, read_at, created_at)| {
            serde_json::json!({
                "id": id,
                "user_address": user_address,
                "notification_type": notification_type,
                "title": title,
                "body": body,
                "data": data,
                "platform_id": platform_id,
                "read_at": read_at,
                "created_at": created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!(result)))
}

pub async fn mark_notification_read(
    Extension(ctx): Extension<RelayContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let notification_id: i64 = match id.parse() {
        Ok(n) => n,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get notification details before marking as read
    let notification: Option<(String, Option<String>)> = relay_notifications::table
        .filter(relay_notifications::id.eq(notification_id))
        .select((
            relay_notifications::user_address,
            relay_notifications::platform_id,
        ))
        .first(&mut conn)
        .await
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if notification.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let (user_address, platform_id) = notification.unwrap();

    // Check if already read
    let is_read: Vec<Option<chrono::DateTime<chrono::Utc>>> = relay_notifications::table
        .filter(relay_notifications::id.eq(notification_id))
        .select(relay_notifications::read_at)
        .load(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let is_read = is_read.into_iter().next().flatten();

    if is_read.is_some() {
        return Ok(Json(serde_json::json!({"status": "already_read"})));
    }

    // Mark as read
    match diesel::update(relay_notifications::table)
        .filter(relay_notifications::id.eq(notification_id))
        .set(relay_notifications::read_at.eq(Some(Utc::now())))
        .execute(&mut conn)
        .await
    {
        Ok(_) => {}
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    }

    // Decrement unread counts
    let mut redis_conn = match get_connection(&ctx.redis_pool).await {
        Ok(c) => c,
        Err(_) => return Ok(Json(serde_json::json!({"status": "ok", "warning": "counts_not_updated"}))),
    };

    // Decrement total count
    let total_key = format!("UNREAD:{}", user_address);
    let _: Result<i64, _> = redis::cmd("DECR")
        .arg(&total_key)
        .query_async(&mut redis_conn)
        .await;

    // Decrement platform-specific count if platform_id exists
    if let Some(pid) = platform_id {
        let platform_key = format!("UNREAD:{}:{}", user_address, pid);
        let _: Result<i64, _> = redis::cmd("DECR")
            .arg(&platform_key)
            .query_async(&mut redis_conn)
            .await;
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}

#[derive(Deserialize)]
pub struct NotificationCountQuery {
    pub user_address: String,
    #[serde(default)]
    pub platform_id: Option<String>,
}

pub async fn get_notification_counts(
    Extension(ctx): Extension<RelayContext>,
    Query(params): Query<NotificationCountQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    
    let mut redis_conn = match get_connection(&ctx.redis_pool).await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Get total unread count
    let total_key = format!("UNREAD:{}", params.user_address);
    let total_count: i64 = match redis::cmd("GET")
        .arg(&total_key)
        .query_async(&mut redis_conn)
        .await
    {
        Ok(v) => v,
        Err(_) => 0,
    };

    let mut result = serde_json::json!({
        "total_unread": total_count.max(0),
    });

    // Get platform-specific count if platform_id is provided
    if let Some(platform_id) = &params.platform_id {
        let platform_key = format!("UNREAD:{}:{}", params.user_address, platform_id);
        let platform_count: i64 = match redis::cmd("GET")
            .arg(&platform_key)
            .query_async(&mut redis_conn)
            .await
        {
            Ok(v) => v,
            Err(_) => 0,
        };
        
        result["platform_unread"] = serde_json::json!(platform_count.max(0));
    } else {
        // If no platform_id specified, get counts for all platforms
        // This requires scanning Redis keys, which is expensive, so we'll use a pattern
        let pattern = format!("UNREAD:{}:*", params.user_address);
        let keys: Vec<String> = match redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut redis_conn)
            .await
        {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };

        let mut platform_counts = serde_json::Map::new();
        for key in keys {
            if let Some(platform_id) = key.strip_prefix(&format!("UNREAD:{}:", params.user_address)) {
                let count: i64 = match redis::cmd("GET")
                    .arg(&key)
                    .query_async(&mut redis_conn)
                    .await
                {
                    Ok(v) => v,
                    Err(_) => 0,
                };
                platform_counts.insert(platform_id.to_string(), serde_json::json!(count.max(0)));
            }
        }
        result["platform_counts"] = serde_json::Value::Object(platform_counts);
    }

    Ok(Json(result))
}

pub async fn get_messages(
    Extension(ctx): Extension<RelayContext>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // TODO: Implement message retrieval
    Ok(Json(serde_json::json!([])))
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub recipient_address: String,
    pub content: String,
}

pub async fn send_message(
    Extension(ctx): Extension<RelayContext>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // TODO: Implement message sending
    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn get_conversations(
    Extension(ctx): Extension<RelayContext>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // TODO: Implement conversation retrieval
    Ok(Json(serde_json::json!([])))
}

pub async fn get_preferences(
    Extension(ctx): Extension<RelayContext>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // TODO: Implement preference retrieval
    Ok(Json(serde_json::json!({})))
}

#[derive(Deserialize)]
pub struct UpdatePreferencesRequest {
    pub push_enabled: Option<bool>,
    pub email_enabled: Option<bool>,
}

pub async fn update_preferences(
    Extension(ctx): Extension<RelayContext>,
    Json(req): Json<UpdatePreferencesRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // TODO: Implement preference update
    Ok(Json(serde_json::json!({"status": "ok"})))
}

#[derive(Deserialize)]
pub struct RegisterDeviceTokenRequest {
    pub device_token: String,
    pub platform: String,
    pub device_id: Option<String>,
}

pub async fn register_device_token(
    Extension(ctx): Extension<RelayContext>,
    Json(req): Json<RegisterDeviceTokenRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // TODO: Implement device token registration
    Ok(Json(serde_json::json!({"status": "ok"})))
}

