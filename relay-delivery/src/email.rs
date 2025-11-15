use anyhow::{Result, anyhow};
use relay_core::config::DeliveryConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing;

/// Simple HTML escaping function
fn html_escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '&' => "&amp;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#x27;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

const RESEND_API_URL: &str = "https://api.resend.com/emails";

#[derive(Debug, Serialize)]
struct ResendEmailRequest {
    from: String,
    to: Vec<String>,
    subject: String,
    html: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResendEmailResponse {
    id: String,
}

pub struct EmailDelivery {
    client: Option<Arc<reqwest::Client>>,
    api_key: Option<String>,
    from_email: Option<String>,
}

impl EmailDelivery {
    pub fn new(config: &DeliveryConfig) -> Result<Self> {
        let (client, api_key, from_email) = if let (Some(api_key), Some(from_email)) = (
            &config.resend_api_key,
            &config.resend_from_email,
        ) {
            tracing::info!("Initializing Resend email client");
            
            // Create HTTP client with proper configuration
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;
            
            tracing::info!("Resend email client initialized successfully");
            (Some(Arc::new(client)), Some(api_key.clone()), Some(from_email.clone()))
        } else {
            tracing::warn!("Email delivery disabled (missing Resend configuration)");
            (None, None, None)
        };

        Ok(Self {
            client,
            api_key,
            from_email,
        })
    }

    pub async fn send(&self, user_address: &str, notification: &Value) -> Result<()> {
        let (client, api_key, from_email) = match (&self.client, &self.api_key, &self.from_email) {
            (Some(c), Some(k), Some(f)) => (c, k, f),
            _ => {
                tracing::debug!("Email not configured, skipping");
                return Ok(());
            }
        };

        // Extract notification fields from the JSON value
        let subject = notification
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Notification");
        
        let body = notification
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("You have a new notification");

        // Build HTML email content
        let html_content = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif; line-height: 1.6; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
    <div style="background-color: #f8f9fa; border-radius: 8px; padding: 24px; margin-bottom: 20px;">
        <h1 style="margin: 0 0 16px 0; font-size: 24px; color: #212529;">{}</h1>
        <p style="margin: 0; font-size: 16px; color: #495057;">{}</p>
    </div>
    <p style="font-size: 14px; color: #6c757d; margin-top: 20px;">
        This is a notification from MySocial.
    </p>
</body>
</html>"#,
            html_escape(subject),
            html_escape(body)
        );

        // Build the Resend API request
        let email_request = ResendEmailRequest {
            from: from_email.clone(),
            to: vec![user_address.to_string()],
            subject: subject.to_string(),
            html: html_content,
            text: Some(body.to_string()),
        };

        // Send the email via Resend API
        let response = client
            .post(RESEND_API_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&email_request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send HTTP request to Resend: {}", e))?;

        // Check response status
        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Resend API returned error status {}: {}",
                status,
                error_text
            ));
        }

        // Parse response to get email ID
        let email_response: ResendEmailResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse Resend API response: {}", e))?;

        tracing::debug!(
            "Email sent successfully via Resend to {} (email_id: {})",
            user_address,
            email_response.id
        );

        Ok(())
    }
}
