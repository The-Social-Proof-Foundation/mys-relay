use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::Json,
};
use relay_core::{
    RelayContext, redis::get_connection, schema::{relay_notifications, relay_messages, relay_conversations, profiles},
    decrypt_message, encrypt_message, verify_mysocial_signature, validate_auth_message,
};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::Utc;
use base64::{engine::general_purpose::STANDARD, Engine};
use crate::auth::AuthenticatedUser;

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "relay-api"
    }))
}

#[derive(Deserialize)]
pub struct AuthRequest {
    pub wallet_address: String,
    pub signature: String,  // Required: MySocial signature (GenericSignature format)
    pub message: String,   // Required: the message that was signed (must include nonce and timestamp)
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub expires_in: u64, // seconds
}

/// Generate JWT token for wallet address
/// Requires valid MySocial signature verification and wallet address must exist in database
pub async fn generate_token(
    Extension(ctx): Extension<RelayContext>,
    Json(req): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, StatusCode> {
    // Normalize wallet address (MySocial addresses are case-sensitive, but we'll normalize for comparison)
    let wallet_address = req.wallet_address.trim();

    // 1. Verify signature matches wallet address using MySocial SDK
    let signature_valid = verify_mysocial_signature(&req.message, &req.signature, wallet_address)
        .await
        .map_err(|e| {
            tracing::warn!("Signature verification failed: {}", e);
            StatusCode::UNAUTHORIZED
        })?;

    if !signature_valid {
        tracing::warn!("Invalid signature for wallet: {}", wallet_address);
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 2. Validate message format and timestamp (prevent replay attacks)
    // Max age: 5 minutes (300 seconds)
    validate_auth_message(&req.message, wallet_address, 300)
        .map_err(|e| {
            tracing::warn!("Message validation failed: {}", e);
            StatusCode::BAD_REQUEST
        })?;

    // 3. Verify wallet address exists in profiles database
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Query profiles table (case-insensitive comparison)
    // Use ILIKE for case-insensitive comparison in PostgreSQL
    let profile_exists: Option<i32> = profiles::table
        .filter(profiles::owner_address.ilike(wallet_address))
        .select(profiles::id)
        .first(&mut conn)
        .await
        .optional()
        .map_err(|e| {
            tracing::error!("Database error checking profile: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if profile_exists.is_none() {
        tracing::warn!("Wallet address not found in database: {}", wallet_address);
        return Err(StatusCode::FORBIDDEN);
    }

    // All checks passed - generate JWT token (expires in 30 days)
    let token = crate::auth::generate_token(wallet_address, &ctx.config.server.jwt_secret, 30)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!("Generated JWT token for wallet: {}", wallet_address);

    Ok(Json(AuthResponse {
        token,
        expires_in: 30 * 24 * 60 * 60, // 30 days in seconds
    }))
}

#[derive(Deserialize)]
pub struct NotificationQuery {
    #[serde(default)]
    pub platform_id: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

pub async fn get_notifications(
    Extension(ctx): Extension<RelayContext>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<NotificationQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(100);
    let offset = params.offset.unwrap_or(0);
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut query = relay_notifications::table
        .filter(relay_notifications::user_address.eq(&user.user_address))
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
    Extension(user): Extension<AuthenticatedUser>,
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

    // Get notification details and verify ownership
    let notification: Option<(String, Option<String>)> = relay_notifications::table
        .filter(relay_notifications::id.eq(notification_id))
        .filter(relay_notifications::user_address.eq(&user.user_address))
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

    let (_user_address, platform_id) = notification.unwrap();

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
    let total_key = format!("UNREAD:{}", user.user_address);
    let _: Result<i64, _> = redis::cmd("DECR")
        .arg(&total_key)
        .query_async(&mut redis_conn)
        .await;

    // Decrement platform-specific count if platform_id exists
    if let Some(pid) = platform_id {
        let platform_key = format!("UNREAD:{}:{}", user.user_address, pid);
        let _: Result<i64, _> = redis::cmd("DECR")
            .arg(&platform_key)
            .query_async(&mut redis_conn)
            .await;
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}

#[derive(Deserialize)]
pub struct NotificationCountQuery {
    #[serde(default)]
    pub platform_id: Option<String>,
}

pub async fn get_notification_counts(
    Extension(ctx): Extension<RelayContext>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<NotificationCountQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut redis_conn = match get_connection(&ctx.redis_pool).await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Get total unread count
    let total_key = format!("UNREAD:{}", user.user_address);
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
        let platform_key = format!("UNREAD:{}:{}", user.user_address, platform_id);
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
        let pattern = format!("UNREAD:{}:*", user.user_address);
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
            if let Some(platform_id) = key.strip_prefix(&format!("UNREAD:{}:", user.user_address)) {
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

#[derive(Deserialize)]
pub struct GetMessagesQuery {
    pub conversation_id: String,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

pub async fn get_messages(
    Extension(ctx): Extension<RelayContext>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<GetMessagesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(100);
    let offset = params.offset.unwrap_or(0);
    
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Verify user is part of the conversation
    let conversation: Option<(String, String)> = relay_conversations::table
        .filter(relay_conversations::conversation_id.eq(&params.conversation_id))
        .select((
            relay_conversations::participant1_address,
            relay_conversations::participant2_address,
        ))
        .first(&mut conn)
        .await
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (p1, p2) = match conversation {
        Some(c) => c,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Verify user is a participant
    if p1 != user.user_address && p2 != user.user_address {
        return Err(StatusCode::FORBIDDEN);
    }

    // Get messages
    let messages: Vec<(i64, String, String, String, Vec<u8>, String, Option<serde_json::Value>, Option<serde_json::Value>, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>)> = relay_messages::table
        .filter(relay_messages::conversation_id.eq(&params.conversation_id))
        .order(relay_messages::created_at.desc())
        .limit(limit)
        .offset(offset)
        .select((
            relay_messages::id,
            relay_messages::conversation_id,
            relay_messages::sender_address,
            relay_messages::recipient_address,
            relay_messages::content,
            relay_messages::content_type,
            relay_messages::media_urls,
            relay_messages::metadata,
            relay_messages::created_at,
            relay_messages::delivered_at,
            relay_messages::read_at,
        ))
        .load(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Decrypt messages
    let mut decrypted_messages = Vec::new();
    for (id, conv_id, sender, recipient, encrypted_content, content_type, media_urls, metadata, created_at, delivered_at, read_at) in messages {
        // Convert BYTEA to base64 string
        let encrypted_base64 = STANDARD.encode(&encrypted_content);
        
        // Decrypt content
        let decrypted_content = decrypt_message(
            &encrypted_base64,
            &conv_id,
            &ctx.config.server.encryption_key,
        ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        decrypted_messages.push(serde_json::json!({
            "id": id,
            "conversation_id": conv_id,
            "sender_address": sender,
            "recipient_address": recipient,
            "content": decrypted_content,
            "content_type": content_type,
            "media_urls": media_urls,
            "metadata": metadata,
            "created_at": created_at,
            "delivered_at": delivered_at,
            "read_at": read_at,
        }));
    }

    Ok(Json(serde_json::json!(decrypted_messages)))
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub recipient_address: String,
    pub content: String,
}

pub async fn send_message(
    Extension(ctx): Extension<RelayContext>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    
    // Create conversation ID
    let (p1, p2) = if user.user_address < req.recipient_address {
        (&user.user_address, &req.recipient_address)
    } else {
        (&req.recipient_address, &user.user_address)
    };
    let conversation_id = format!("{}:{}", p1, p2);

    // Encrypt message
    let encrypted_content = encrypt_message(
        &req.content,
        &conversation_id,
        &ctx.config.server.encryption_key,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let encrypted_bytes = STANDARD.decode(&encrypted_content)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Ensure conversation exists
    let exists: Option<i64> = relay_conversations::table
        .filter(relay_conversations::conversation_id.eq(&conversation_id))
        .select(relay_conversations::id)
        .first(&mut conn)
        .await
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if exists.is_none() {
        diesel::insert_into(relay_conversations::table)
            .values((
                relay_conversations::conversation_id.eq(&conversation_id),
                relay_conversations::participant1_address.eq(p1),
                relay_conversations::participant2_address.eq(p2),
            ))
            .execute(&mut conn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    // Insert message
    diesel::insert_into(relay_messages::table)
        .values((
            relay_messages::conversation_id.eq(&conversation_id),
            relay_messages::sender_address.eq(&user.user_address),
            relay_messages::recipient_address.eq(&req.recipient_address),
            relay_messages::content.eq(encrypted_bytes),
            relay_messages::content_type.eq("text"),
        ))
        .execute(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update conversation timestamp
    diesel::update(relay_conversations::table.filter(relay_conversations::conversation_id.eq(&conversation_id)))
        .set(relay_conversations::last_message_at.eq(Utc::now()))
        .execute(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Emit to Redpanda for WebSocket delivery
    use relay_core::redpanda::produce_message;
    let event_data = serde_json::json!({
        "sender_address": user.user_address,
        "recipient_address": req.recipient_address,
        "content": req.content,
        "conversation_id": conversation_id,
    });
    let payload_bytes = serde_json::to_vec(&event_data).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = produce_message(&ctx.redpanda_producer, "events.message.created", Some(user.user_address.as_str()), &payload_bytes).await;

    Ok(Json(serde_json::json!({"status": "ok", "conversation_id": conversation_id})))
}

#[derive(Deserialize)]
pub struct GetConversationsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

pub async fn get_conversations(
    Extension(ctx): Extension<RelayContext>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<GetConversationsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(100);
    let offset = params.offset.unwrap_or(0);
    
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Get conversations where user is a participant
    let conversations: Vec<(String, String, String, Option<chrono::DateTime<chrono::Utc>>, chrono::DateTime<chrono::Utc>)> = relay_conversations::table
        .filter(
            relay_conversations::participant1_address.eq(&user.user_address)
                .or(relay_conversations::participant2_address.eq(&user.user_address))
        )
        .order(relay_conversations::last_message_at.desc().nulls_last())
        .limit(limit)
        .offset(offset)
        .select((
            relay_conversations::conversation_id,
            relay_conversations::participant1_address,
            relay_conversations::participant2_address,
            relay_conversations::last_message_at,
            relay_conversations::created_at,
        ))
        .load(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result: Vec<serde_json::Value> = conversations
        .into_iter()
        .map(|(conv_id, p1, p2, last_message_at, created_at)| {
            // Determine the other participant
            let other_participant = if p1 == user.user_address { p2 } else { p1 };
            
            serde_json::json!({
                "conversation_id": conv_id,
                "other_participant": other_participant,
                "last_message_at": last_message_at,
                "created_at": created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!(result)))
}

pub async fn get_preferences(
    Extension(ctx): Extension<RelayContext>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    use relay_core::schema::relay_user_preferences;
    let prefs: Option<(bool, bool, bool, serde_json::Value)> = relay_user_preferences::table
        .filter(relay_user_preferences::user_address.eq(&user.user_address))
        .select((
            relay_user_preferences::push_enabled,
            relay_user_preferences::email_enabled,
            relay_user_preferences::sms_enabled,
            relay_user_preferences::notification_types,
        ))
        .first(&mut conn)
        .await
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match prefs {
        Some((push_enabled, email_enabled, sms_enabled, notification_types)) => {
            Ok(Json(serde_json::json!({
                "push_enabled": push_enabled,
                "email_enabled": email_enabled,
                "sms_enabled": sms_enabled,
                "notification_types": notification_types,
            })))
        }
        None => Ok(Json(serde_json::json!({
            "push_enabled": true,
            "email_enabled": true,
            "sms_enabled": false,
            "notification_types": serde_json::json!({}),
        })))
    }
}

#[derive(Deserialize)]
pub struct UpdatePreferencesRequest {
    pub push_enabled: Option<bool>,
    pub email_enabled: Option<bool>,
    pub sms_enabled: Option<bool>,
    pub notification_types: Option<serde_json::Value>,
}

pub async fn update_preferences(
    Extension(ctx): Extension<RelayContext>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<UpdatePreferencesRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    use relay_core::schema::relay_user_preferences;
    
    // Get existing preferences or use defaults
    let existing: Option<(bool, bool, bool, serde_json::Value)> = relay_user_preferences::table
        .filter(relay_user_preferences::user_address.eq(&user.user_address))
        .select((
            relay_user_preferences::push_enabled,
            relay_user_preferences::email_enabled,
            relay_user_preferences::sms_enabled,
            relay_user_preferences::notification_types,
        ))
        .first(&mut conn)
        .await
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (push_enabled, email_enabled, sms_enabled, notification_types) = match existing {
        Some((p, e, s, n)) => (
            req.push_enabled.unwrap_or(p),
            req.email_enabled.unwrap_or(e),
            req.sms_enabled.unwrap_or(s),
            req.notification_types.clone().unwrap_or(n),
        ),
        None => (
            req.push_enabled.unwrap_or(true),
            req.email_enabled.unwrap_or(true),
            req.sms_enabled.unwrap_or(false),
            req.notification_types.clone().unwrap_or_else(|| serde_json::json!({})),
        ),
    };

    // Upsert preferences
    diesel::insert_into(relay_user_preferences::table)
        .values((
            relay_user_preferences::user_address.eq(&user.user_address),
            relay_user_preferences::push_enabled.eq(push_enabled),
            relay_user_preferences::email_enabled.eq(email_enabled),
            relay_user_preferences::sms_enabled.eq(sms_enabled),
            relay_user_preferences::notification_types.eq(&notification_types),
            relay_user_preferences::updated_at.eq(Utc::now()),
        ))
        .on_conflict(relay_user_preferences::user_address)
        .do_update()
        .set((
            relay_user_preferences::push_enabled.eq(push_enabled),
            relay_user_preferences::email_enabled.eq(email_enabled),
            relay_user_preferences::sms_enabled.eq(sms_enabled),
            relay_user_preferences::notification_types.eq(&notification_types),
            relay_user_preferences::updated_at.eq(Utc::now()),
        ))
        .execute(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<RegisterDeviceTokenRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut conn = match ctx.db_pool.get().await {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    use relay_core::schema::relay_device_tokens;
    
    // Upsert device token
    diesel::insert_into(relay_device_tokens::table)
        .values((
            relay_device_tokens::user_address.eq(&user.user_address),
            relay_device_tokens::device_token.eq(&req.device_token),
            relay_device_tokens::platform.eq(&req.platform),
            relay_device_tokens::device_id.eq(req.device_id.as_deref()),
            relay_device_tokens::last_used_at.eq(Utc::now()),
        ))
        .on_conflict((relay_device_tokens::user_address, relay_device_tokens::device_token))
        .do_update()
        .set((
            relay_device_tokens::platform.eq(&req.platform),
            relay_device_tokens::device_id.eq(req.device_id.as_deref()),
            relay_device_tokens::last_used_at.eq(Utc::now()),
            relay_device_tokens::updated_at.eq(Utc::now()),
        ))
        .execute(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({"status": "ok"})))
}

