use anyhow::{Result, anyhow};
use a2::{Client, NotificationBuilder, PlainNotificationBuilder, NotificationOptions};
use relay_core::config::DeliveryConfig;
use serde_json::Value;
use std::fs;
use tracing;

pub struct ApnsDelivery {
    client: Option<Client>,
    bundle_id: String,
}

impl ApnsDelivery {
    pub fn new(config: &DeliveryConfig) -> Result<Self> {
        let bundle_id = config.apns_bundle_id.clone().unwrap_or_default();
        
        let client = if let (Some(key_id), Some(team_id)) = (
            &config.apns_key_id,
            &config.apns_team_id,
        ) {
            tracing::info!("Initializing APNs client");
            
            // Read the key file or use base64 content if provided
            let key_content = if let Some(key_content_base64) = &config.apns_key_content {
                // Decode base64 encoded key content
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(key_content_base64)
                    .map_err(|e| anyhow!("Failed to decode base64 APNs key: {}", e))?;
                String::from_utf8(decoded)
                    .map_err(|e| anyhow!("Failed to convert APNs key to UTF-8: {}", e))?
            } else if let Some(key_path) = &config.apns_key_path {
                fs::read_to_string(key_path)
                    .map_err(|e| anyhow!("Failed to read APNs key file {}: {}", key_path, e))?
            } else {
                return Err(anyhow!("Either apns_key_path or apns_key_content must be provided"));
            };
            
            // Create APNs client
            let client = Client::token(
                key_content.as_bytes(),
                key_id,
                team_id,
                if bundle_id.contains("sandbox") || bundle_id.contains("dev") {
                    a2::Endpoint::Sandbox
                } else {
                    a2::Endpoint::Production
                },
            )
            .map_err(|e| anyhow!("Failed to create APNs client: {}", e))?;
            
            tracing::info!("APNs client initialized successfully");
            Some(client)
        } else {
            tracing::warn!("APNs delivery disabled (missing configuration)");
            None
        };

        Ok(Self {
            client,
            bundle_id,
        })
    }

    pub async fn send(&self, device_token: &str, notification: &Value) -> Result<()> {
        let client = match &self.client {
            Some(c) => c,
            None => {
                tracing::debug!("APNs not configured, skipping");
                return Ok(());
            }
        };

        // Extract notification fields from the JSON value
        let body = notification
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("You have a new notification");

        // Build the notification payload using PlainNotificationBuilder
        let mut builder = PlainNotificationBuilder::new(body);
        
        // Optionally set badge, sound, category if present in notification data
        if let Some(badge) = notification.get("badge").and_then(|v| v.as_u64()) {
            builder.set_badge(badge as u32);
        }
        
        if let Some(sound) = notification.get("sound").and_then(|v| v.as_str()) {
            builder.set_sound(sound);
        }
        
        if let Some(category) = notification.get("category").and_then(|v| v.as_str()) {
            builder.set_category(category);
        }
        
        // Set notification options with topic (bundle ID) - required for token-based auth
        let mut options = NotificationOptions::default();
        if !self.bundle_id.is_empty() {
            options.apns_topic = Some(&self.bundle_id);
        }
        
        // Build the notification payload
        let payload = builder.build(device_token, options);

        // Send the notification
        let response = client.send(payload).await
            .map_err(|e| anyhow!("Failed to send APNs notification: {}", e))?;

        tracing::debug!(
            "APNs notification sent successfully to device {}: {:?}",
            device_token,
            response
        );
        
        Ok(())
    }
}
