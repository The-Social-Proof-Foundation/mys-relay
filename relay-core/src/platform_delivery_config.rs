use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use crate::schema::platform_delivery_config;
use crate::db::DbConnection;

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = platform_delivery_config)]
pub struct PlatformDeliveryConfig {
    pub id: i64,
    pub platform_id: String,
    pub apns_bundle_id: Option<String>,
    pub apns_key_id: Option<String>,
    pub apns_team_id: Option<String>,
    pub apns_key_path: Option<String>,
    pub apns_key_content: Option<String>,
    pub fcm_server_key: Option<String>,
    pub resend_api_key: Option<String>,
    pub resend_from_email: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Insertable, Serialize, Deserialize)]
#[diesel(table_name = platform_delivery_config)]
pub struct NewPlatformDeliveryConfig {
    pub platform_id: String,
    pub apns_bundle_id: Option<String>,
    pub apns_key_id: Option<String>,
    pub apns_team_id: Option<String>,
    pub apns_key_path: Option<String>,
    pub apns_key_content: Option<String>,
    pub fcm_server_key: Option<String>,
    pub resend_api_key: Option<String>,
    pub resend_from_email: Option<String>,
}

/// Get platform delivery configuration, falling back to None if not found
pub async fn get_platform_delivery_config(
    conn: &mut DbConnection,
    platform_id: &str,
) -> anyhow::Result<Option<PlatformDeliveryConfig>> {
    use crate::schema::platform_delivery_config;
    
    let configs: Vec<PlatformDeliveryConfig> = diesel_async::RunQueryDsl::load(
        platform_delivery_config::table
            .filter(platform_delivery_config::platform_id.eq(platform_id))
            .limit(1),
        &mut *conn
    )
    .await?;
    
    Ok(configs.into_iter().next())
}

/// Convert platform delivery config to DeliveryConfig format for compatibility
impl From<&PlatformDeliveryConfig> for crate::config::DeliveryConfig {
    fn from(config: &PlatformDeliveryConfig) -> Self {
        crate::config::DeliveryConfig {
            apns_bundle_id: config.apns_bundle_id.clone(),
            apns_key_id: config.apns_key_id.clone(),
            apns_team_id: config.apns_team_id.clone(),
            apns_key_path: config.apns_key_path.clone(),
            apns_key_content: config.apns_key_content.clone(),
            fcm_server_key: config.fcm_server_key.clone(),
            resend_api_key: config.resend_api_key.clone(),
            resend_from_email: config.resend_from_email.clone(),
        }
    }
}

