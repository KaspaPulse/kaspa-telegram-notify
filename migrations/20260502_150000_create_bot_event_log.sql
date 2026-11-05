CREATE TABLE IF NOT EXISTS bot_event_log (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    event_type TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'info',

    chat_id BIGINT NULL,
    user_name TEXT NULL,

    command TEXT NULL,
    callback_data TEXT NULL,

    wallet_masked TEXT NULL,
    txid_masked TEXT NULL,
    block_hash_masked TEXT NULL,

    status TEXT NULL,
    error_message TEXT NULL,

    duration_ms BIGINT NULL,

    metadata JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_created_at
ON bot_event_log (created_at DESC);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_event_type
ON bot_event_log (event_type);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_chat_id
ON bot_event_log (chat_id);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_status
ON bot_event_log (status);
