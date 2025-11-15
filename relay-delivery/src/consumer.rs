use anyhow::{Result, anyhow};
use rdkafka::consumer::Consumer;
use rdkafka::Message;
use relay_core::{RelayContext, redpanda::create_consumer, get_platform_delivery_config};
use crate::{apns::ApnsDelivery, fcm::FcmDelivery, email::EmailDelivery};
use std::time::Duration;
use tracing;

const TOPIC: &str = "notifications.delivery";

pub async fn run(ctx: RelayContext) -> Result<()> {
    tracing::info!("Starting delivery consumer");

    let consumer = create_consumer(&ctx.config.redpanda, Some("relay-delivery"))?;
    
    // Global fallback delivery clients (for MySocial platform or when platform config not found)
    let global_apns = ApnsDelivery::new(&ctx.config.delivery)?;
    let global_fcm = FcmDelivery::new(&ctx.config.delivery)?;
    let global_email = EmailDelivery::new(&ctx.config.delivery)?;

    consumer.subscribe(&[TOPIC])?;

    tracing::info!("Subscribed to topic: {}", TOPIC);

    let mut error_count = 0u32;
    let mut last_error_log = std::time::Instant::now();
    
    loop {
        match consumer.recv().await {
            Ok(message) => {
                error_count = 0; // Reset error count on success
                if let Some(payload) = message.payload() {
                    match handle_delivery(&ctx, &global_apns, &global_fcm, &global_email, payload).await {
                        Ok(_) => {
                            tracing::debug!("Processed delivery job");
                        }
                        Err(e) => {
                            tracing::error!("Error processing delivery job: {}", e);
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

async fn handle_delivery(
    ctx: &RelayContext,
    global_apns: &ApnsDelivery,
    global_fcm: &FcmDelivery,
    global_email: &EmailDelivery,
    payload: &[u8],
) -> Result<()> {
    let job: serde_json::Value = serde_json::from_slice(payload)?;
    
    let user_address = job.get("user_address")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing user_address"))?;

    // Extract platform_id from job (if available)
    let platform_id = job.get("platform_id")
        .and_then(|v| v.as_str());

    // Get device tokens for user
    let mut conn = ctx.db_pool.get().await?;
    use relay_core::schema::relay_device_tokens;
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;
    
    let tokens: Vec<(String, String)> = relay_device_tokens::table
        .filter(relay_device_tokens::user_address.eq(user_address))
        .select((relay_device_tokens::device_token, relay_device_tokens::platform))
        .load(&mut conn)
        .await
        .unwrap_or_default();

    let notification = job.get("notification")
        .ok_or_else(|| anyhow::anyhow!("Missing notification"))?;

    // Get platform-specific delivery config if platform_id is provided
    if let Some(pid) = platform_id {
        match get_platform_delivery_config(&mut conn, pid).await {
            Ok(Some(platform_config)) => {
                tracing::debug!("Using platform-specific delivery config for platform: {}", pid);
                let delivery_config = relay_core::config::DeliveryConfig::from(&platform_config);
                
                // Create platform-specific clients
                if let (Ok(platform_apns), Ok(platform_fcm), Ok(platform_email)) = (
                    ApnsDelivery::new(&delivery_config),
                    FcmDelivery::new(&delivery_config),
                    EmailDelivery::new(&delivery_config),
                ) {
                    // Use platform-specific clients
                    for (token, platform) in &tokens {
                        match platform.as_str() {
                            "ios" => {
                                if let Err(e) = platform_apns.send(token, notification).await {
                                    tracing::error!("Failed to send platform APNs notification: {}", e);
                                }
                            }
                            "android" => {
                                if let Err(e) = platform_fcm.send(token, notification).await {
                                    tracing::error!("Failed to send platform FCM notification: {}", e);
                                }
                            }
                            _ => {}
                        }
                    }
                    
                    // Send email if enabled
                    if let Err(e) = platform_email.send(user_address, notification).await {
                        tracing::error!("Failed to send platform email notification: {}", e);
                    }
                    
                    return Ok(());
                } else {
                    tracing::warn!("Failed to create platform delivery clients, falling back to global");
                }
            }
            Ok(None) => {
                tracing::debug!("No platform-specific config found for platform: {}, using global", pid);
            }
            Err(e) => {
                tracing::warn!("Error fetching platform config, using global: {}", e);
            }
        }
    }

    // Use global clients (fallback or when no platform_id)
    for (token, platform) in tokens {
        match platform.as_str() {
            "ios" => {
                if let Err(e) = global_apns.send(&token, notification).await {
                    tracing::error!("Failed to send APNs notification: {}", e);
                }
            }
            "android" => {
                if let Err(e) = global_fcm.send(&token, notification).await {
                    tracing::error!("Failed to send FCM notification: {}", e);
                }
            }
            _ => {}
        }
    }

    // Send email if enabled
    if let Err(e) = global_email.send(user_address, notification).await {
        tracing::error!("Failed to send email notification: {}", e);
    }

    Ok(())
}

