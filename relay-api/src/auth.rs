use axum::{
    extract::Request,
    http::{header::AUTHORIZATION, StatusCode},
    response::Response,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use relay_core::RelayContext;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing;

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub user_address: String,
    pub exp: usize,
}

/// Authenticated user information
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_address: String,
}

/// Extract JWT token from Authorization header
fn extract_token(auth_header: Option<&str>) -> Option<String> {
    auth_header?
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
}

/// Generate JWT token for a user address
pub fn generate_token(user_address: &str, secret: &str, expires_in_days: u64) -> Result<String, StatusCode> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .as_secs() as usize;
    
    let exp = now + (expires_in_days * 24 * 60 * 60) as usize; // Convert days to seconds
    
    let claims = Claims {
        user_address: user_address.to_string(),
        exp,
    };
    
    let encoding_key = EncodingKey::from_secret(secret.as_ref());
    
    encode(&Header::default(), &claims, &encoding_key)
        .map_err(|e| {
            tracing::error!("Failed to generate JWT token: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

/// Verify JWT token and extract user address
pub fn verify_token(token: &str, secret: &str) -> Result<String, StatusCode> {
    let decoding_key = DecodingKey::from_secret(secret.as_ref());
    let validation = Validation::default();

    match decode::<Claims>(token, &decoding_key, &validation) {
        Ok(token_data) => Ok(token_data.claims.user_address),
        Err(e) => {
            tracing::debug!("JWT verification failed: {}", e);
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/// Axum middleware for JWT authentication
pub async fn auth_middleware(
    mut req: Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    // Skip authentication for health check, WebSocket, and auth endpoints
    let path = req.uri().path();
    if path == "/health" || path.starts_with("/ws") || path == "/api/v1/auth/token" {
        return Ok(next.run(req).await);
    }

    // Extract Authorization header
    let auth_header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let token = match extract_token(auth_header) {
        Some(t) => t,
        None => {
            tracing::debug!("Missing Authorization header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Get JWT secret from context
    let ctx = req
        .extensions()
        .get::<RelayContext>()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let user_address = verify_token(&token, &ctx.config.server.jwt_secret)?;

    // Add authenticated user to request extensions
    req.extensions_mut().insert(AuthenticatedUser {
        user_address: user_address.clone(),
    });

    tracing::debug!("Authenticated user: {}", user_address);

    Ok(next.run(req).await)
}

/// Extract authenticated user from request extensions
pub fn get_authenticated_user(req: &Request) -> Result<AuthenticatedUser, StatusCode> {
    req.extensions()
        .get::<AuthenticatedUser>()
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)
}

