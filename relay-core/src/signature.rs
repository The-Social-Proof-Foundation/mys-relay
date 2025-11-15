use anyhow::{Result, anyhow};
use mys_sdk::verify_personal_message_signature::verify_personal_message_signature;
use mys_types::{
    Address,
    GenericSignature,
};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// Verify MySocial signature using mys-sdk
/// This uses the custom MySocial signature format, not Ethereum's
pub async fn verify_mysocial_signature(
    message: &str,
    signature: &str,
    expected_address: &str,
) -> Result<bool> {
    // Parse signature string to GenericSignature (expects JSON format)
    let generic_sig: GenericSignature = serde_json::from_str(signature)
        .map_err(|e| anyhow!("Failed to parse signature as JSON: {}", e))?;

    // Parse wallet address to Address
    let mys_address = Address::from_str(expected_address)
        .map_err(|e| anyhow!("Failed to parse wallet address: {}", e))?;

    // Convert message string to bytes
    let message_bytes = message.as_bytes();

    // Verify signature using mys-sdk
    // Note: For zkLogin signatures, we would need a MysClient, but for standard signatures we can pass None
    match verify_personal_message_signature(generic_sig, message_bytes, mys_address, None).await {
        Ok(_) => Ok(true),
        Err(e) => {
            tracing::debug!("Signature verification failed: {}", e);
            Ok(false)
        }
    }
}

/// Validate message contains nonce/timestamp to prevent replay attacks
/// Expected format: "Sign in to MySocial Relay\n\nWallet: {address}\nNonce: {nonce}\nTimestamp: {timestamp}"
pub fn validate_auth_message(message: &str, wallet_address: &str, max_age_seconds: u64) -> Result<()> {
    // Check message format
    if !message.contains("Sign in to MySocial Relay") {
        return Err(anyhow!("Invalid message format: missing expected prefix"));
    }

    if !message.contains(&format!("Wallet: {}", wallet_address)) {
        return Err(anyhow!("Message does not contain expected wallet address"));
    }

    // Extract timestamp
    let timestamp_str = message
        .lines()
        .find(|line| line.starts_with("Timestamp:"))
        .and_then(|line| line.split("Timestamp:").nth(1))
        .ok_or_else(|| anyhow!("Missing timestamp in message"))?
        .trim();

    let timestamp: u64 = timestamp_str
        .parse()
        .map_err(|_| anyhow!("Invalid timestamp format"))?;

    // Check timestamp is not too old
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow!("Failed to get current time"))?
        .as_secs();

    if timestamp > now {
        return Err(anyhow!("Timestamp is in the future"));
    }

    if now - timestamp > max_age_seconds {
        return Err(anyhow!("Message is too old (max age: {} seconds)", max_age_seconds));
    }

    // Extract nonce (optional but recommended)
    if !message.contains("Nonce:") {
        tracing::warn!("Message missing nonce - replay protection may be limited");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_validation() {
        let wallet = "0x1234567890123456789012345678901234567890";
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let message = format!(
            "Sign in to MySocial Relay\n\nWallet: {}\nNonce: abc123\nTimestamp: {}",
            wallet, timestamp
        );

        assert!(validate_auth_message(&message, wallet, 300).is_ok());
    }
}

