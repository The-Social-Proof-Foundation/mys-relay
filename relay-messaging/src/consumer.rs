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

    loop {
        match consumer.recv().await {
            Ok(message) => {
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
                tracing::error!("Error receiving message: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
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

