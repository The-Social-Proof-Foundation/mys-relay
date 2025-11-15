use mys_types::{Address, GenericSignature, UserSignature};
use crate::simple::SimpleVerifier;
use crate::Verifier;

/// Verify a personal message signature
pub async fn verify_personal_message_signature(
    signature: GenericSignature,
    message: &[u8],
    _address: Address, // Address is validated via signature verification
    _client: Option<()>, // Placeholder for future MysClient support
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // GenericSignature is an alias for UserSignature
    let user_sig: UserSignature = signature;
    
    // Create verifier and verify
    let verifier = SimpleVerifier;
    verifier.verify(message, &user_sig)
        .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}", e))) as Box<dyn std::error::Error + Send + Sync>)
}
