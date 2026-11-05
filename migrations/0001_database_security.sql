-- =============================================================================
-- Kaspa Pulse migration 0001
-- Purpose:
--   Create/ensure security-sensitive tables and explicit dedup constraints.
-- Notes:
--   This migration is intentionally idempotent.
-- =============================================================================

CREATE TABLE IF NOT EXISTS user_wallets (
    wallet TEXT NOT NULL,
    chat_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_active TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (wallet, chat_id)
);

CREATE TABLE IF NOT EXISTS mined_blocks (
    id BIGSERIAL PRIMARY KEY,
    wallet TEXT NOT NULL,
    outpoint TEXT NOT NULL,
    amount BIGINT NOT NULL,
    daa_score BIGINT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS wallet_alert_dedup (
    wallet TEXT NOT NULL,
    alert_key TEXT NOT NULL,
    txid_masked TEXT,
    block_hash_masked TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (wallet, alert_key)
);

CREATE TABLE IF NOT EXISTS wallet_seen_utxos (
    wallet TEXT NOT NULL,
    outpoint TEXT NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (wallet, outpoint)
);

CREATE TABLE IF NOT EXISTS pending_rewards (
    wallet TEXT NOT NULL,
    outpoint TEXT NOT NULL,
    txid TEXT NOT NULL,
    amount BIGINT NOT NULL,
    reward_daa_score BIGINT NOT NULL,
    virtual_daa_score BIGINT NOT NULL,
    confirmations BIGINT NOT NULL DEFAULT 0,
    required_confirmations BIGINT NOT NULL DEFAULT 10,
    attempts BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_checked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (wallet, outpoint)
);

CREATE TABLE IF NOT EXISTS bot_event_log (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    chat_id BIGINT,
    user_name TEXT,
    command TEXT,
    callback_data TEXT,
    wallet_masked TEXT,
    txid_masked TEXT,
    block_hash_masked TEXT,
    status TEXT,
    error_message TEXT,
    duration_ms BIGINT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_user_wallets_wallet_chat
    ON user_wallets (wallet, chat_id);

CREATE UNIQUE INDEX IF NOT EXISTS ux_mined_blocks_outpoint
    ON mined_blocks (outpoint);

CREATE UNIQUE INDEX IF NOT EXISTS ux_wallet_alert_dedup_wallet_alert_key
    ON wallet_alert_dedup (wallet, alert_key);

CREATE UNIQUE INDEX IF NOT EXISTS ux_wallet_seen_utxos_wallet_outpoint
    ON wallet_seen_utxos (wallet, outpoint);

CREATE UNIQUE INDEX IF NOT EXISTS ux_pending_rewards_wallet_outpoint
    ON pending_rewards (wallet, outpoint);

CREATE INDEX IF NOT EXISTS idx_user_wallets_chat_id
    ON user_wallets (chat_id);

CREATE INDEX IF NOT EXISTS idx_mined_blocks_wallet_timestamp
    ON mined_blocks (wallet, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_wallet_alert_dedup_created_at
    ON wallet_alert_dedup (created_at);

CREATE INDEX IF NOT EXISTS idx_wallet_seen_utxos_last_seen_at
    ON wallet_seen_utxos (last_seen_at);

CREATE INDEX IF NOT EXISTS idx_pending_rewards_status_checked
    ON pending_rewards (status, last_checked_at);

CREATE INDEX IF NOT EXISTS idx_pending_rewards_wallet
    ON pending_rewards (wallet);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_created_at
    ON bot_event_log (created_at);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_event_type_created_at
    ON bot_event_log (event_type, created_at DESC);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'ck_pending_rewards_status'
    ) THEN
        ALTER TABLE pending_rewards
        ADD CONSTRAINT ck_pending_rewards_status
        CHECK (status IN ('pending', 'confirmed', 'failed', 'expired'));
    END IF;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'ck_bot_event_log_severity'
    ) THEN
        ALTER TABLE bot_event_log
        ADD CONSTRAINT ck_bot_event_log_severity
        CHECK (severity IN ('info', 'warn', 'error'));
    END IF;
END $$;
