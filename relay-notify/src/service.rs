use anyhow::{Result, anyhow};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use relay_core::schema::relay_notifications;
use relay_core::{RelayContext, redis::get_connection};
use serde_json::Value;
use tracing;

pub struct NotificationService {
    ctx: RelayContext,
}

impl NotificationService {
    pub fn new(ctx: RelayContext) -> Self {
        Self { ctx }
    }

    pub async fn process_event(&self, event_type: &str, event_data: &Value) -> Result<()> {
        tracing::debug!("Processing notification event: {}", event_type);

        // Extract user addresses from event data
        let recipients = self.extract_recipients(event_type, event_data)?;

        for recipient in recipients {
            // Check user preferences
            if !self.should_notify(&recipient, event_type).await? {
                continue;
            }

            // Create notification
            let notification = self.create_notification(event_type, event_data, &recipient).await?;

            // Store in Redis inbox
            self.add_to_redis_inbox(&recipient, &notification).await?;

            // Increment unread count
            self.increment_unread_count(&recipient).await?;

            // Emit delivery job to Redpanda
            self.emit_delivery_job(&recipient, &notification).await?;
        }

        Ok(())
    }

    fn extract_recipients(&self, event_type: &str, event_data: &Value) -> Result<Vec<String>> {
        match event_type {
            "like.created" | "comment.created" => {
                if let Some(post_owner) = event_data.get("post_owner").and_then(|v| v.as_str()) {
                    Ok(vec![post_owner.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "follow.created" => {
                if let Some(following) = event_data.get("following_address").and_then(|v| v.as_str()) {
                    Ok(vec![following.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "message.created" => {
                if let Some(recipient) = event_data.get("recipient_address").and_then(|v| v.as_str()) {
                    Ok(vec![recipient.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            _ => Ok(vec![]),
        }
    }

    async fn should_notify(&self, user_address: &str, event_type: &str) -> Result<bool> {
        // TODO: Check user preferences from database
        // For now, default to true
        Ok(true)
    }

    async fn create_notification(
        &self,
        event_type: &str,
        event_data: &Value,
        user_address: &str,
    ) -> Result<Value> {
        let (title, body) = self.format_notification(event_type, event_data);

        let notification = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "user_address": user_address,
            "notification_type": event_type,
            "title": title,
            "body": body,
            "data": event_data,
            "created_at": Utc::now(),
        });

        // Store in Postgres
        let mut conn = self.ctx.db_pool.get().await?;
        diesel::insert_into(relay_notifications::table)
            .values((
                relay_notifications::user_address.eq(user_address),
                relay_notifications::notification_type.eq(event_type),
                relay_notifications::title.eq(&title),
                relay_notifications::body.eq(&body),
                relay_notifications::data.eq(event_data),
            ))
            .execute(&mut conn)
            .await?;

        Ok(notification)
    }

    fn format_notification(&self, event_type: &str, event_data: &Value) -> (String, String) {
        match event_type {
            "like.created" => (
                "New Like".to_string(),
                format!("Someone liked your post"),
            ),
            "comment.created" => (
                "New Comment".to_string(),
                format!("Someone commented on your post"),
            ),
            "follow.created" => (
                "New Follower".to_string(),
                format!("Someone started following you"),
            ),
            "message.created" => (
                "New Message".to_string(),
                format!("You have a new message"),
            ),
            _ => (
                "Notification".to_string(),
                format!("You have a new notification"),
            ),
        }
    }

    async fn add_to_redis_inbox(&self, user_address: &str, notification: &Value) -> Result<()> {
        let mut conn = get_connection(&self.ctx.redis_pool).await?;
        let key = format!("INBOX:{}", user_address);
        
        redis::cmd("LPUSH")
            .arg(&key)
            .arg(serde_json::to_string(notification)?)
            .query_async(&mut conn)
            .await?;

        // Keep only last 100 notifications
        redis::cmd("LTRIM")
            .arg(&key)
            .arg(0)
            .arg(99)
            .query_async(&mut conn)
            .await?;

        Ok(())
    }

    async fn increment_unread_count(&self, user_address: &str) -> Result<()> {
        let mut conn = get_connection(&self.ctx.redis_pool).await?;
        let key = format!("UNREAD:{}", user_address);
        
        redis::cmd("INCR")
            .arg(&key)
            .query_async(&mut conn)
            .await?;

        Ok(())
    }

    async fn emit_delivery_job(&self, user_address: &str, notification: &Value) -> Result<()> {
        // Extract platform_id from notification data if available
        let platform_id = notification
            .get("data")
            .and_then(|d| d.get("platform_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut payload = serde_json::json!({
            "user_address": user_address,
            "notification": notification,
        });
        
        if let Some(pid) = platform_id {
            payload["platform_id"] = serde_json::Value::String(pid);
        }

        let payload_bytes = serde_json::to_vec(&payload)?;
        relay_core::redpanda::produce_message(
            &self.ctx.redpanda_producer,
            "notifications.delivery",
            Some(user_address),
            &payload_bytes,
        )
        .await?;

        Ok(())
    }
}

