use anyhow::{Result, anyhow};
use rdkafka::consumer::Consumer;
use rdkafka::Message;
use relay_core::{RelayContext, redpanda::create_consumer};
use crate::service::MessagingService;
use std::time::Duration;
use tracing;

const TOPIC: &str = "events.message.created";

pub async fn run(ctx: RelayContext) -> Result<()> {
    tracing::info!("Starting messaging consumer");

    let consumer = create_consumer(&ctx.config.redpanda, Some("relay-messaging"))?;
    let service = MessagingService::new(ctx.clone());

    consumer.subscribe(&[TOPIC])?;

    tracing::info!("Subscribed to topic: {}", TOPIC);

    let mut error_count = 0u32;
    let mut last_error_log = std::time::Instant::now();
    
    loop {
        match consumer.recv().await {
            Ok(message) => {
                error_count = 0; // Reset error count on success
                if let Some(payload) = message.payload() {
                    match handle_message(&service, payload).await {
                        Ok(_) => {
                            tracing::debug!("Processed message event");
                        }
                        Err(e) => {
                            tracing::error!("Error processing message event: {}", e);
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

async fn handle_message(service: &MessagingService, payload: &[u8]) -> Result<()> {
    let event: serde_json::Value = serde_json::from_slice(payload)?;
    
    let event_data = event.get("event_data")
        .ok_or_else(|| anyhow::anyhow!("Missing event_data"))?;

    service.process_message(event_data).await?;

    Ok(())
}

