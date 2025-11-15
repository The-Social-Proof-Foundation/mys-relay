use anyhow::{Result, anyhow};
use rdkafka::consumer::Consumer;
use rdkafka::Message;
use relay_core::{RelayContext, redpanda::create_consumer};
use crate::service::NotificationService;
use std::time::Duration;
use tracing;

const TOPICS: &[&str] = &[
    // Post-related events
    "events.post.reaction",
    "events.post.repost",
    "events.post.tip",
    "events.post.created",
    "events.post.ownership",
    // Comment events
    "events.comment.created",
    // Social proof token events
    "events.spt.created",
    // Governance events
    "events.governance.created",
    // Prediction events
    "events.prediction.created",
    // Social graph events
    "events.follow.created",
    "events.unfollow.created",
    // Platform events
    "events.platform.created",
    // Note: events.message.created is handled by relay-messaging service, not here
];

pub async fn run(ctx: RelayContext) -> Result<()> {
    tracing::info!("Starting notification consumer");

    let consumer = create_consumer(&ctx.config.redpanda, Some("relay-notify"))?;
    let service = NotificationService::new(ctx.clone());

    consumer.subscribe(TOPICS)?;

    tracing::info!("Subscribed to topics: {:?}", TOPICS);

    let mut error_count = 0u32;
    let mut last_error_log = std::time::Instant::now();
    
    loop {
        match consumer.recv().await {
            Ok(message) => {
                error_count = 0; // Reset error count on success
                if let Some(payload) = message.payload() {
                    match handle_event(&service, payload).await {
                        Ok(_) => {
                            tracing::debug!("Processed notification event");
                        }
                        Err(e) => {
                            tracing::error!("Error processing notification event: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                error_count += 1;
                // Only log errors every 30 seconds to reduce log spam
                if last_error_log.elapsed().as_secs() >= 30 {
                    tracing::warn!(
                        "Error receiving message from Redpanda (error count: {}): {}",
                        error_count,
                        e
                    );
                    last_error_log = std::time::Instant::now();
                }
                // Exponential backoff: 1s, 2s, 4s, max 30s
                let backoff = Duration::from_secs(1 << error_count.min(5)).min(Duration::from_secs(30));
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

async fn handle_event(service: &NotificationService, payload: &[u8]) -> Result<()> {
    let event: serde_json::Value = serde_json::from_slice(payload)?;
    
    let event_type = event.get("event_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing event_type"))?;

    let event_data = event.get("event_data")
        .ok_or_else(|| anyhow::anyhow!("Missing event_data"))?;

    service.process_event(event_type, event_data).await?;

    Ok(())
}

