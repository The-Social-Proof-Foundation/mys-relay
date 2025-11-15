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
            
            // Extract platform_id for counting
            let platform_id = notification
                .get("platform_id")
                .and_then(|v| v.as_str());

            // Store in Redis inbox
            self.add_to_redis_inbox(&recipient, &notification).await?;

            // Increment unread count (total and platform-specific)
            self.increment_unread_count(&recipient, platform_id).await?;

            // Emit delivery job to Redpanda
            self.emit_delivery_job(&recipient, &notification).await?;
        }

        Ok(())
    }

    fn extract_recipients(&self, event_type: &str, event_data: &Value) -> Result<Vec<String>> {
        match event_type {
            // Post-related events
            "reaction.created" | "comment.created" => {
                if let Some(post_owner) = event_data.get("post_owner").and_then(|v| v.as_str()) {
                    Ok(vec![post_owner.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "repost.created" => {
                if let Some(post_owner) = event_data.get("post_owner").and_then(|v| v.as_str()) {
                    Ok(vec![post_owner.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "tip.created" => {
                if let Some(recipient) = event_data.get("recipient").and_then(|v| v.as_str()) {
                    Ok(vec![recipient.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "post.created" => {
                // Post created events may notify followers (future enhancement)
                // For now, we don't notify on post creation
                Ok(vec![])
            }
            "ownership.transferred" => {
                if let Some(new_owner) = event_data.get("new_owner").and_then(|v| v.as_str()) {
                    Ok(vec![new_owner.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            // Social graph events
            "follow.created" => {
                if let Some(following) = event_data.get("following_address").and_then(|v| v.as_str()) {
                    Ok(vec![following.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "unfollow.created" => {
                if let Some(following) = event_data.get("following_address").and_then(|v| v.as_str()) {
                    Ok(vec![following.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            // Social proof token events
            "spt.token_bought" | "spt.token_sold" | "spt.tokens_added" => {
                if let Some(pool_owner) = event_data.get("pool_owner").and_then(|v| v.as_str()) {
                    Ok(vec![pool_owner.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "spt.reservation_created" => {
                if let Some(associated_owner) = event_data.get("associated_owner").and_then(|v| v.as_str()) {
                    Ok(vec![associated_owner.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            // Governance events
            "governance.proposal_submitted" => {
                // Notify delegates/platform admins (complex - would need DB lookup)
                // For now, return empty - can be enhanced later
                Ok(vec![])
            }
            "governance.proposal_approved" | "governance.proposal_rejected" | 
            "governance.proposal_rejected_by_community" | "governance.proposal_implemented" => {
                if let Some(submitter) = event_data.get("submitter").and_then(|v| v.as_str()) {
                    Ok(vec![submitter.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            // Prediction events
            "prediction.bet_placed" | "prediction.resolved" => {
                if let Some(post_owner) = event_data.get("post_owner").and_then(|v| v.as_str()) {
                    Ok(vec![post_owner.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "prediction.payout" => {
                if let Some(recipient) = event_data.get("recipient").and_then(|v| v.as_str()) {
                    Ok(vec![recipient.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            // Platform events
            "platform.moderator_added" | "platform.moderator_removed" => {
                if let Some(moderator_address) = event_data.get("moderator_address").and_then(|v| v.as_str()) {
                    Ok(vec![moderator_address.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            "platform.user_joined" | "platform.user_left" => {
                // Notify platform moderators/owners (would need DB lookup)
                // For now, return empty - can be enhanced later
                Ok(vec![])
            }
            // Messaging (handled separately by messaging service)
            "message.created" => {
                if let Some(recipient) = event_data.get("recipient_address").and_then(|v| v.as_str()) {
                    Ok(vec![recipient.to_string()])
                } else {
                    Ok(vec![])
                }
            }
            _ => {
                tracing::warn!("Unknown event type for recipient extraction: {}", event_type);
                Ok(vec![])
            }
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
        
        // Extract platform_id from event data if available
        let platform_id = event_data
            .get("platform_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let notification = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "user_address": user_address,
            "notification_type": event_type,
            "title": title,
            "body": body,
            "data": event_data,
            "platform_id": platform_id,
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
                relay_notifications::platform_id.eq(platform_id.as_deref()),
            ))
            .execute(&mut conn)
            .await?;

        Ok(notification)
    }

    fn format_notification(&self, event_type: &str, event_data: &Value) -> (String, String) {
        match event_type {
            // Post-related events
            "reaction.created" => {
                let reaction = event_data.get("reaction").and_then(|v| v.as_str()).unwrap_or("reacted");
                (
                    "New Reaction".to_string(),
                    format!("Someone {} to your post", reaction),
                )
            }
            "repost.created" => {
                let reposter = event_data.get("reposter").and_then(|v| v.as_str()).unwrap_or("Someone");
                (
                    "New Repost".to_string(),
                    format!("{} reposted your post", reposter),
                )
            }
            "tip.created" => {
                let amount = event_data.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                let tipper = event_data.get("tipper").and_then(|v| v.as_str()).unwrap_or("Someone");
                (
                    "New Tip".to_string(),
                    format!("{} tipped you {} MYSO", tipper, amount),
                )
            }
            "post.created" => {
                // Post created events typically don't notify the creator
                (
                    "Post Created".to_string(),
                    "Your post was created".to_string(),
                )
            }
            "ownership.transferred" => {
                (
                    "Ownership Transferred".to_string(),
                    "You are now the owner of this post".to_string(),
                )
            }
            "comment.created" => {
                let commenter = event_data.get("commenter").and_then(|v| v.as_str()).unwrap_or("Someone");
                (
                    "New Comment".to_string(),
                    format!("{} commented on your post", commenter),
                )
            }
            // Social graph events
            "follow.created" => {
                (
                    "New Follower".to_string(),
                    "Someone started following you".to_string(),
                )
            }
            "unfollow.created" => {
                (
                    "User Unfollowed".to_string(),
                    "Someone unfollowed you".to_string(),
                )
            }
            // Social proof token events
            "spt.token_bought" => {
                let buyer = event_data.get("buyer").and_then(|v| v.as_str()).unwrap_or("Someone");
                let amount = event_data.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                (
                    "Token Bought".to_string(),
                    format!("{} bought {} tokens from your pool", buyer, amount),
                )
            }
            "spt.token_sold" => {
                let seller = event_data.get("seller").and_then(|v| v.as_str()).unwrap_or("Someone");
                let amount = event_data.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                (
                    "Token Sold".to_string(),
                    format!("{} sold {} tokens from your pool", seller, amount),
                )
            }
            "spt.tokens_added" => {
                let amount = event_data.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                (
                    "Tokens Added".to_string(),
                    format!("{} tokens were added to your pool", amount),
                )
            }
            "spt.reservation_created" => {
                let reserver = event_data.get("reserver").and_then(|v| v.as_str()).unwrap_or("Someone");
                let amount = event_data.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                (
                    "New Reservation".to_string(),
                    format!("{} reserved {} tokens", reserver, amount),
                )
            }
            // Governance events
            "governance.proposal_submitted" => {
                (
                    "New Proposal".to_string(),
                    "A new governance proposal was submitted".to_string(),
                )
            }
            "governance.proposal_approved" => {
                (
                    "Proposal Approved".to_string(),
                    "Your governance proposal was approved".to_string(),
                )
            }
            "governance.proposal_rejected" => {
                (
                    "Proposal Rejected".to_string(),
                    "Your governance proposal was rejected".to_string(),
                )
            }
            "governance.proposal_rejected_by_community" => {
                (
                    "Proposal Rejected".to_string(),
                    "Your governance proposal was rejected by the community".to_string(),
                )
            }
            "governance.proposal_implemented" => {
                (
                    "Proposal Implemented".to_string(),
                    "Your governance proposal was implemented".to_string(),
                )
            }
            // Prediction events
            "prediction.bet_placed" => {
                let bettor = event_data.get("bettor").and_then(|v| v.as_str()).unwrap_or("Someone");
                let amount = event_data.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                (
                    "New Bet".to_string(),
                    format!("{} placed a bet of {} MYSO on your prediction", bettor, amount),
                )
            }
            "prediction.resolved" => {
                (
                    "Prediction Resolved".to_string(),
                    "Your prediction has been resolved".to_string(),
                )
            }
            "prediction.payout" => {
                let amount = event_data.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                (
                    "Prediction Payout".to_string(),
                    format!("You received {} MYSO from your prediction bet", amount),
                )
            }
            // Platform events
            "platform.moderator_added" => {
                (
                    "Moderator Added".to_string(),
                    "You were added as a platform moderator".to_string(),
                )
            }
            "platform.moderator_removed" => {
                (
                    "Moderator Removed".to_string(),
                    "You were removed as a platform moderator".to_string(),
                )
            }
            "platform.user_joined" => {
                (
                    "User Joined Platform".to_string(),
                    "A new user joined your platform".to_string(),
                )
            }
            "platform.user_left" => {
                (
                    "User Left Platform".to_string(),
                    "A user left your platform".to_string(),
                )
            }
            // Messaging (handled separately by messaging service)
            "message.created" => {
                (
                    "New Message".to_string(),
                    "You have a new message".to_string(),
                )
            }
            _ => {
                tracing::warn!("Unknown event type for notification formatting: {}", event_type);
                (
                    "Notification".to_string(),
                    "You have a new notification".to_string(),
                )
            }
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

    async fn increment_unread_count(&self, user_address: &str, platform_id: Option<&str>) -> Result<()> {
        let mut conn = get_connection(&self.ctx.redis_pool).await?;
        
        // Increment total unread count
        let total_key = format!("UNREAD:{}", user_address);
        redis::cmd("INCR")
            .arg(&total_key)
            .query_async(&mut conn)
            .await?;
        
        // Increment platform-specific unread count if platform_id is provided
        if let Some(pid) = platform_id {
            let platform_key = format!("UNREAD:{}:{}", user_address, pid);
            redis::cmd("INCR")
                .arg(&platform_key)
                .query_async(&mut conn)
                .await?;
        }

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

