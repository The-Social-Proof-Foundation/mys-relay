use diesel::{table, allow_tables_to_appear_in_same_query};

table! {
    relay_outbox (id) {
        id -> BigInt,
        event_type -> Text,
        event_data -> Jsonb,
        event_id -> Nullable<Text>,
        transaction_id -> Nullable<Text>,
        created_at -> Timestamptz,
        processed_at -> Nullable<Timestamptz>,
        published_at -> Nullable<Timestamptz>,
        retry_count -> Integer,
        error_message -> Nullable<Text>,
    }
}

table! {
    relay_notifications (id) {
        id -> BigInt,
        user_address -> Text,
        notification_type -> Text,
        title -> Text,
        body -> Text,
        data -> Nullable<Jsonb>,
        read_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

table! {
    relay_messages (id) {
        id -> BigInt,
        conversation_id -> Text,
        sender_address -> Text,
        recipient_address -> Text,
        content -> Text,
        content_type -> Text,
        media_urls -> Nullable<Jsonb>,
        metadata -> Nullable<Jsonb>,
        created_at -> Timestamptz,
        delivered_at -> Nullable<Timestamptz>,
        read_at -> Nullable<Timestamptz>,
    }
}

table! {
    relay_conversations (id) {
        id -> BigInt,
        conversation_id -> Text,
        participant1_address -> Text,
        participant2_address -> Text,
        last_message_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

table! {
    relay_user_preferences (user_address) {
        user_address -> Text,
        push_enabled -> Bool,
        email_enabled -> Bool,
        sms_enabled -> Bool,
        notification_types -> Jsonb,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

table! {
    relay_device_tokens (id) {
        id -> BigInt,
        user_address -> Text,
        device_token -> Text,
        platform -> Text,
        device_id -> Nullable<Text>,
        app_version -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        last_used_at -> Timestamptz,
    }
}

table! {
    relay_ws_connections (id) {
        id -> BigInt,
        user_address -> Text,
        connection_id -> Text,
        connected_at -> Timestamptz,
        last_heartbeat_at -> Timestamptz,
        disconnected_at -> Nullable<Timestamptz>,
    }
}

table! {
    platform_delivery_config (id) {
        id -> BigInt,
        platform_id -> Text,
        apns_bundle_id -> Nullable<Text>,
        apns_key_id -> Nullable<Text>,
        apns_team_id -> Nullable<Text>,
        apns_key_path -> Nullable<Text>,
        apns_key_content -> Nullable<Text>,
        fcm_server_key -> Nullable<Text>,
        resend_api_key -> Nullable<Text>,
        resend_from_email -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

allow_tables_to_appear_in_same_query!(
    relay_outbox,
    relay_notifications,
    relay_messages,
    relay_conversations,
    relay_user_preferences,
    relay_device_tokens,
    relay_ws_connections,
    platform_delivery_config,
);

