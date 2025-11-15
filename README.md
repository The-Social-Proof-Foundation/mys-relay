# MySocial Relay Server

A production-ready notification and messaging relay server for the MySocial platform. The relay server handles real-time notifications, messaging, push notifications (APNs/FCM), email delivery, and WebSocket connections.

## Architecture

The relay server follows a modular, single-binary architecture where all services run as parallel Tokio tasks within one `relay-runner` binary:

```
relay-runner (main binary)
├── relay-core (shared config, DB pool, Redis pool, types, context)
├── relay-api (REST API server + WebSocket server)
├── relay-notify (notification processing service)
├── relay-messaging (messaging service)
├── relay-delivery (APNs/FCM/Email delivery workers)
└── relay-outbox (CDC poller for Postgres → Redpanda)
```

### Key Components

- **Outbox Poller**: Polls `relay_outbox` table (written by indexer) and publishes events to Redpanda
- **Notification Service**: Consumes notification events from Redpanda, stores in Postgres/Redis, and triggers delivery
- **Messaging Service**: Handles direct messages between users
- **Delivery Service**: Sends push notifications (APNs/FCM) and emails (Resend) to users
- **API Server**: REST API for notifications, messages, preferences, and WebSocket for real-time updates

## Features

### Notifications
- ✅ **Platform-specific**: Notifications are tied to specific platforms
- ✅ Real-time notification processing from blockchain events
- ✅ Platform-specific notification filtering
- ✅ Per-user and per-platform unread notification counts
- ✅ Redis-backed inbox for fast retrieval
- ✅ Postgres persistence for historical data
- ✅ WebSocket support for real-time updates

### Messaging
- ✅ **Platform-agnostic**: Direct messaging between users (not platform-specific)
- ✅ Conversation tracking
- ✅ Redis Streams for real-time message delivery
- ✅ Message read receipts
- ✅ Messages work across all platforms - users can message each other regardless of platform context

### Delivery
- ✅ **Platform-specific delivery configuration**: Each platform can configure its own APNs, FCM, and email settings
- ✅ **APNs (iOS)**: Token-based authentication with support for key file or base64-encoded key content
- ✅ **FCM (Android)**: Firebase Cloud Messaging integration
- ✅ **Email (Resend)**: Direct API integration for email delivery
- ✅ Fallback to global delivery config when platform config is missing

### Platform Configuration

The relay server supports platform-specific delivery configuration stored in the `platform_delivery_config` table:

- **APNs**: `apns_bundle_id`, `apns_key_id`, `apns_team_id`, `apns_key_path` or `apns_key_content` (base64)
- **FCM**: `fcm_server_key`
- **Resend**: `resend_api_key`, `resend_from_email`

When a notification includes a `platform_id`, the relay server:
1. Looks up platform-specific delivery configuration
2. Creates platform-specific delivery clients
3. Falls back to global config if platform config is missing

## Database Schema

### Core Tables

- `relay_outbox`: CDC table written by indexer, polled by relay
- `relay_notifications`: User notifications with platform_id support (platform-specific)
- `relay_messages`: Direct messages between users (platform-agnostic)
- `relay_conversations`: Conversation metadata (platform-agnostic)
- `relay_user_preferences`: User notification preferences
- `relay_device_tokens`: Device tokens for push notifications
- `relay_ws_connections`: Active WebSocket connections
- `platform_delivery_config`: Platform-specific delivery settings

### Platform-Specific vs Platform-Agnostic

- **Platform-Specific**: Notifications are tied to specific platforms and can be filtered by `platform_id`
- **Platform-Agnostic**: Messages and conversations work across all platforms - users can message each other regardless of platform context

## Redis Keys

- `INBOX:{user_address}`: List of recent notifications (last 100)
- `UNREAD:{user_address}`: Total unread notification count
- `UNREAD:{user_address}:{platform_id}`: Platform-specific unread count
- `CHAT:{conversation_id}`: Conversation messages
- `STREAM:CHAT:{user_address}`: Redis Stream for real-time message delivery

## Redpanda Topics

The relay server uses a category-based topic structure for organizing notification events:

### Notification Event Topics

- **Post-related events:**
  - `events.post.reaction`: `reaction.created`
  - `events.post.repost`: `repost.created`
  - `events.post.tip`: `tip.created`
  - `events.post.created`: `post.created`
  - `events.post.ownership`: `ownership.transferred`

- **Comment events:**
  - `events.comment.created`: `comment.created`

- **Social proof token events:**
  - `events.spt.created`: `spt.token_bought`, `spt.token_sold`, `spt.tokens_added`, `spt.reservation_created`

- **Governance events:**
  - `events.governance.created`: `governance.proposal_submitted`, `governance.proposal_approved`, `governance.proposal_rejected`, `governance.proposal_rejected_by_community`, `governance.proposal_implemented`

- **Prediction events:**
  - `events.prediction.created`: `prediction.bet_placed`, `prediction.resolved`, `prediction.payout`

- **Social graph events:**
  - `events.follow.created`: `follow.created`
  - `events.unfollow.created`: `unfollow.created`

- **Platform events:**
  - `events.platform.created`: `platform.moderator_added`, `platform.moderator_removed`, `platform.user_joined`, `platform.user_left`

- **Messaging events:**
  - `events.message.created`: `message.created` (handled by messaging service, not notification service)

### Delivery Topics

- `notifications.delivery`: Delivery jobs (consumed by delivery workers)
- `delivery.apns`: APNs delivery queue (legacy, not currently used)
- `delivery.fcm`: FCM delivery queue (legacy, not currently used)
- `delivery.email`: Email delivery queue (legacy, not currently used)

### Topic Routing

Events are routed to topics based on their `event_type` prefix:
- `reaction.*` → `events.post.reaction`
- `repost.*` → `events.post.repost`
- `tip.*` → `events.post.tip`
- `post.created` → `events.post.created`
- `ownership.transferred` → `events.post.ownership`
- `comment.*` → `events.comment.created`
- `spt.*` → `events.spt.created`
- `governance.*` → `events.governance.created`
- `prediction.*` → `events.prediction.created`
- `follow.*` → `events.follow.created`
- `unfollow.*` → `events.unfollow.created`
- `platform.*` → `events.platform.created`
- `message.*` → `events.message.created`
- Unknown events → `events.unknown` (with warning)

## API Endpoints

All authenticated endpoints require a valid JWT token in the `Authorization` header as `Bearer {token}`.

- `POST /api/v1/auth/token`: Generate JWT token (requires MySocial signature verification, no auth required)
- `GET /api/v1/notifications?platform_id={pid}&limit={n}&offset={n}`: Get notifications (requires JWT auth, supports platform filtering)
- `GET /api/v1/notifications/counts?platform_id={pid}`: Get unread notification counts (requires JWT auth, total and per-platform)
- `POST /api/v1/notifications/:id/read`: Mark notification as read (requires JWT auth)
- `GET /api/v1/messages?conversation_id={cid}&limit={n}&offset={n}`: Get messages (requires JWT auth, messages are automatically decrypted)
- `POST /api/v1/messages`: Send message (requires JWT auth, message content is automatically encrypted)
- `GET /api/v1/conversations?limit={n}&offset={n}`: Get conversations (requires JWT auth, platform-agnostic)
- `GET /api/v1/preferences`: Get user notification preferences (requires JWT auth)
- `POST /api/v1/preferences`: Update user notification preferences (requires JWT auth)
- `POST /api/v1/device-tokens`: Register/update device token for push notifications (requires JWT auth)
- `GET /ws?token={jwt_token}`: WebSocket connection for real-time updates (requires JWT token in query param)
- `GET /health`: Health check endpoint (no authentication required)

## Configuration

### Environment Variables

#### Database
- `DATABASE_URL`: PostgreSQL connection string
- `DATABASE_MAX_CONNECTIONS`: Max DB connections (default: 10)

#### Redis
- `REDIS_URL`: Redis connection string
- `REDIS_MAX_CONNECTIONS`: Max Redis connections (default: 10)

#### Redpanda/Kafka
- `REDPANDA_BROKERS`: Comma-separated list of brokers (e.g., `localhost:9092`)
- `REDPANDA_CONSUMER_GROUP`: Consumer group name

#### Server
- `API_PORT` or `PORT`: API server port (default: 8080)
- `WS_PORT`: WebSocket port (default: 8081)
- `SERVER_HOST`: Server host (default: 0.0.0.0)
- `JWT_SECRET`: Secret key for JWT token signing (required in production)
- `ENCRYPTION_KEY`: Master encryption key for message encryption (64 hex characters, required in production)

#### Global Delivery Config (Fallback)
- `APNS_BUNDLE_ID`: iOS bundle ID
- `APNS_KEY_ID`: APNs key ID
- `APNS_TEAM_ID`: APNs team ID
- `APNS_KEY_PATH`: Path to APNs .p8 key file (or use `APNS_KEY_CONTENT`)
- `APNS_KEY_CONTENT`: Base64-encoded APNs key content (alternative to `APNS_KEY_PATH`)
- `FCM_SERVER_KEY`: FCM server key
- `RESEND_API_KEY`: Resend API key
- `RESEND_FROM_EMAIL`: Resend sender email address

**Note**: Platform-specific delivery configuration should be stored in the `platform_delivery_config` table. Global config is used as a fallback for MySocial platform notifications or when platform config is missing.

## Setup

### Prerequisites

- Rust 1.81+
- PostgreSQL 14+
- Redis 6+
- Redpanda or Kafka

### Database Migrations

Run migrations to set up the database schema:

```bash
cd crates/mys-social-indexer
diesel migration run
```

### Building

```bash
cd mys-relay
cargo build --release
```

### Running

```bash
cd mys-relay
cargo run --bin relay-runner
```

Or use the Dockerfile:

```bash
docker build -t mys-relay .
docker run -p 8080:8080 mys-relay
```

## Deployment

### Railway

The project includes a `railway.toml` configuration file for Railway deployment:

```toml
[build]
builder = "DOCKERFILE"
dockerfilePath = "Dockerfile"

[deploy]
startCommand = "relay"
healthcheckPath = "/health"
healthcheckTimeout = 100
restartPolicyType = "ON_FAILURE"
restartPolicyMaxRetries = 10
```

**Note**: The Dockerfile expects to be run from the parent directory (`crates/mys-social-indexer`). Set the Railway root directory accordingly or adjust the Dockerfile paths.

## Platform Configuration

To configure delivery settings for a platform:

```sql
INSERT INTO platform_delivery_config (
    platform_id,
    apns_bundle_id,
    apns_key_id,
    apns_team_id,
    apns_key_content,  -- Base64-encoded .p8 key content
    fcm_server_key,
    resend_api_key,
    resend_from_email
) VALUES (
    'your-platform-id',
    'com.example.app',
    'ABC123XYZ',
    'TEAM123',
    'base64-encoded-key-content',
    'fcm-server-key',
    'resend-api-key',
    'noreply@example.com'
);
```

## Notification Flow (Platform-Specific)

1. **Indexer** writes events to `relay_outbox` table (includes platform_id when available)
2. **Outbox Poller** reads unprocessed events and publishes to Redpanda topics
3. **Notification Service** consumes events, extracts platform_id, creates notifications, stores in Postgres/Redis
4. **Notification Service** increments unread counts (total and platform-specific)
5. **Notification Service** emits delivery job to `notifications.delivery` topic (includes platform_id)
6. **Delivery Service** consumes delivery jobs, looks up platform-specific config, and sends via APNs/FCM/Email
7. **API Server** serves notifications via REST API and WebSocket (supports platform filtering)

## Messaging Flow (Platform-Agnostic)

1. **Indexer** writes message events to `relay_outbox` table
2. **Outbox Poller** publishes message events to `events.message.created` topic
3. **Messaging Service** consumes events, encrypts message content, stores in Postgres/Redis
4. **Messaging Service** updates conversation metadata
5. **Messaging Service** emits WebSocket events via Redis Streams
6. **API Server** serves messages via REST API and WebSocket (no platform filtering)
7. **API Server** automatically decrypts messages before returning to clients

**Message Encryption:**
- Messages are encrypted using AES-256-GCM before storage
- Each conversation uses a unique encryption key derived from the master key
- Encryption keys are derived using HKDF with the conversation ID as the salt
- Only the message content is encrypted; metadata (sender, recipient, timestamps) remains unencrypted

## Development

### Project Structure

```
mys-relay/
├── relay-core/          # Shared core functionality
├── relay-api/           # REST API and WebSocket server
├── relay-notify/        # Notification processing
├── relay-messaging/     # Messaging service
├── relay-delivery/      # Delivery workers (APNs/FCM/Email)
├── relay-outbox/        # CDC poller
├── relay-runner/        # Main binary (spawns all services)
├── Cargo.toml          # Workspace configuration
├── Dockerfile          # Docker build configuration
└── railway.toml        # Railway deployment config
```

### Testing

```bash
cargo test --workspace
```

### Linting

```bash
cargo clippy --workspace
```

## Security Considerations

### Authentication
- **JWT Tokens**: Tokens expire after 30 days. Clients should refresh tokens before expiration.
- **Signature Verification**: All token generation requests require valid MySocial signatures.
- **Replay Protection**: Message timestamps prevent replay attacks (5-minute window).
- **Database Validation**: Wallet addresses must exist in the profiles table.

### Message Encryption
- **At-Rest Encryption**: All messages are encrypted before storage in PostgreSQL.
- **Key Derivation**: Per-conversation keys prevent key compromise from affecting other conversations.
- **Key Management**: The master encryption key (`ENCRYPTION_KEY`) must be kept secure and rotated periodically.

### Production Checklist
- [ ] Set strong `JWT_SECRET` (use cryptographically secure random string)
- [ ] Set strong `ENCRYPTION_KEY` (64 hex characters, generate with: `openssl rand -hex 32`)
- [ ] Use HTTPS/TLS for all API connections
- [ ] Configure proper CORS policies
- [ ] Set up monitoring and alerting
- [ ] Rotate encryption keys periodically
- [ ] Implement rate limiting for authentication endpoints
- [ ] Use secure database credentials
- [ ] Enable Redis authentication if exposed

## License

Apache-2.0

