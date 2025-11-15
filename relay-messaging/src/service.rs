use anyhow::{Result, anyhow};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use relay_core::schema::{relay_messages, relay_conversations};
use relay_core::{RelayContext, redis::get_connection, encrypt_message};
use serde_json::Value;
use tracing;
use base64::{engine::general_purpose::STANDARD, Engine};

pub struct MessagingService {
    ctx: RelayContext,
}

impl MessagingService {
    pub fn new(ctx: RelayContext) -> Self {
        Self { ctx }
    }

    pub async fn process_message(&self, event_data: &Value) -> Result<()> {
        let sender = event_data.get("sender_address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing sender_address"))?;

        let recipient = event_data.get("recipient_address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing recipient_address"))?;

        let content = event_data.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing content"))?;

        let conversation_id = self.get_or_create_conversation(sender, recipient).await?;

        // Encrypt message content before storing
        let encrypted_content = encrypt_message(
            content,
            &conversation_id,
            &self.ctx.config.server.encryption_key,
        )?;
        
        // Convert encrypted string to bytes for BYTEA storage
        let encrypted_bytes = STANDARD.decode(&encrypted_content)
            .map_err(|e| anyhow!("Failed to decode encrypted content: {}", e))?;

        // Store encrypted message in Postgres
        let mut conn = self.ctx.db_pool.get().await?;
        diesel::insert_into(relay_messages::table)
            .values((
                relay_messages::conversation_id.eq(&conversation_id),
                relay_messages::sender_address.eq(sender),
                relay_messages::recipient_address.eq(recipient),
                relay_messages::content.eq(encrypted_bytes),
                relay_messages::content_type.eq("text"),
            ))
            .execute(&mut conn)
            .await?;

        // Update conversation
        diesel::update(relay_conversations::table.filter(relay_conversations::conversation_id.eq(&conversation_id)))
            .set(relay_conversations::last_message_at.eq(Utc::now()))
            .execute(&mut conn)
            .await?;

        // Cache in Redis
        self.cache_message(&conversation_id, sender, recipient, content).await?;

        // Emit WebSocket event
        self.emit_ws_event(recipient, &conversation_id, content).await?;

        Ok(())
    }

    async fn get_or_create_conversation(&self, user1: &str, user2: &str) -> Result<String> {
        // Create deterministic conversation ID
        let (p1, p2) = if user1 < user2 {
            (user1, user2)
        } else {
            (user2, user1)
        };
        let conversation_id = format!("{}:{}", p1, p2);

        let mut conn = self.ctx.db_pool.get().await?;

        // Check if exists
        let exists: Option<i64> = relay_conversations::table
            .filter(relay_conversations::conversation_id.eq(&conversation_id))
            .select(relay_conversations::id)
            .first(&mut conn)
            .await
            .optional()?;

        if exists.is_none() {
            diesel::insert_into(relay_conversations::table)
                .values((
                    relay_conversations::conversation_id.eq(&conversation_id),
                    relay_conversations::participant1_address.eq(p1),
                    relay_conversations::participant2_address.eq(p2),
                ))
                .execute(&mut conn)
                .await?;
        }

        Ok(conversation_id)
    }

    async fn cache_message(
        &self,
        conversation_id: &str,
        sender: &str,
        recipient: &str,
        content: &str,
    ) -> Result<()> {
        let mut conn = get_connection(&self.ctx.redis_pool).await?;
        let key = format!("CHAT:{}", conversation_id);

        let message = serde_json::json!({
            "sender": sender,
            "recipient": recipient,
            "content": content,
            "created_at": Utc::now(),
        });

        redis::cmd("LPUSH")
            .arg(&key)
            .arg(serde_json::to_string(&message)?)
            .query_async(&mut conn)
            .await?;

        // Keep only last 50 messages
        redis::cmd("LTRIM")
            .arg(&key)
            .arg(0)
            .arg(49)
            .query_async(&mut conn)
            .await?;

        Ok(())
    }

    async fn emit_ws_event(&self, user_address: &str, conversation_id: &str, content: &str) -> Result<()> {
        let payload = serde_json::json!({
            "type": "message",
            "conversation_id": conversation_id,
            "content": content,
        });

        let payload_bytes = serde_json::to_vec(&payload)?;
        let stream_key = format!("STREAM:CHAT:{}", user_address);

        let mut conn = get_connection(&self.ctx.redis_pool).await?;
        redis::cmd("XADD")
            .arg(&stream_key)
            .arg("*")
            .arg("data")
            .arg(String::from_utf8_lossy(&payload_bytes))
            .query_async(&mut conn)
            .await?;

        Ok(())
    }
}

