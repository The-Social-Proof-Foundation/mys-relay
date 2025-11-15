use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Result, anyhow};
use base64::{engine::general_purpose::STANDARD, Engine};
use hex;
use hkdf::Hkdf;
use sha2::Sha256;

/// Encrypt message content using AES-256-GCM
/// Derives a key from the master encryption key and conversation ID for per-conversation encryption
pub fn encrypt_message(
    content: &str,
    conversation_id: &str,
    master_key: &str,
) -> Result<String> {
    // Derive a conversation-specific key using HKDF
    let key = derive_conversation_key(master_key, conversation_id)?;
    
    // Create cipher
    let cipher = Aes256Gcm::new(&key);
    
    // Generate a random nonce
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    
    // Encrypt the content
    let ciphertext = cipher
        .encrypt(&nonce, content.as_bytes())
        .map_err(|e| anyhow!("Encryption failed: {}", e))?;
    
    // Combine nonce and ciphertext, then base64 encode
    let mut encrypted_data = nonce.to_vec();
    encrypted_data.extend_from_slice(&ciphertext);
    
    Ok(STANDARD.encode(&encrypted_data))
}

/// Decrypt message content using AES-256-GCM
pub fn decrypt_message(
    encrypted_content: &str,
    conversation_id: &str,
    master_key: &str,
) -> Result<String> {
    // Decode base64
    let encrypted_data = STANDARD
        .decode(encrypted_content)
        .map_err(|e| anyhow!("Base64 decode failed: {}", e))?;
    
    if encrypted_data.len() < 12 {
        return Err(anyhow!("Invalid encrypted data: too short"));
    }
    
    // Extract nonce (first 12 bytes) and ciphertext
    let nonce = Nonce::from_slice(&encrypted_data[..12]);
    let ciphertext = &encrypted_data[12..];
    
    // Derive the same conversation-specific key
    let key = derive_conversation_key(master_key, conversation_id)?;
    
    // Create cipher
    let cipher = Aes256Gcm::new(&key);
    
    // Decrypt
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("Decryption failed: {}", e))?;
    
    String::from_utf8(plaintext)
        .map_err(|e| anyhow!("Invalid UTF-8 after decryption: {}", e))
}

/// Derive a conversation-specific encryption key using HKDF
fn derive_conversation_key(master_key: &str, conversation_id: &str) -> Result<Key<Aes256Gcm>> {
    // Decode master key from hex or use directly as bytes
    let master_key_bytes = if master_key.len() == 64 {
        // Assume hex encoding (32 bytes = 64 hex chars)
        hex::decode(master_key)
            .map_err(|e| anyhow!("Invalid hex master key: {}", e))?
    } else {
        // Use as raw bytes (truncate/pad to 32 bytes)
        let mut key_bytes = master_key.as_bytes().to_vec();
        key_bytes.resize(32, 0);
        key_bytes
    };
    
    // Use HKDF to derive a 32-byte key from master key and conversation ID
    let hk = Hkdf::<Sha256>::new(None, &master_key_bytes);
    let mut okm = [0u8; 32];
    hk.expand(conversation_id.as_bytes(), &mut okm)
        .map_err(|e| anyhow!("HKDF expansion failed: {}", e))?;
    
    Ok(*Key::<Aes256Gcm>::from_slice(&okm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let master_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let conversation_id = "conv-123";
        let original = "Hello, this is a secret message!";
        
        let encrypted = encrypt_message(original, conversation_id, master_key).unwrap();
        assert_ne!(encrypted, original);
        
        let decrypted = decrypt_message(&encrypted, conversation_id, master_key).unwrap();
        assert_eq!(decrypted, original);
    }
}

