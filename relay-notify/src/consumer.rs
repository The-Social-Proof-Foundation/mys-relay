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

    loop {
        match consumer.recv().await {
            Ok(message) => {
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
                tracing::error!("Error receiving message: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
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

