use anyhow::{Result, anyhow};
use fcm::Client;
use relay_core::config::DeliveryConfig;
use serde_json::Value;
use tracing;

pub struct FcmDelivery {
    client: Option<Client>,
    server_key: Option<String>,
}

impl FcmDelivery {
    pub fn new(config: &DeliveryConfig) -> Result<Self> {
        let (client, server_key) = if let Some(key) = &config.fcm_server_key {
            tracing::info!("Initializing FCM client");
            
            let client = Client::new();
            
            tracing::info!("FCM client initialized successfully");
            (Some(client), Some(key.clone()))
        } else {
            tracing::warn!("FCM delivery disabled (missing configuration)");
            (None, None)
        };

        Ok(Self { client, server_key })
    }

    pub async fn send(&self, device_token: &str, notification: &Value) -> Result<()> {
        if self.client.is_none() || self.server_key.is_none() {
            tracing::debug!("FCM not configured, skipping");
            return Ok(());
        }

        // TODO: Implement actual FCM delivery
        // The fcm 0.9 crate API needs to be checked for the correct usage
        tracing::debug!("Would send FCM notification to device: {}", device_token);
        Ok(())
    }
}
