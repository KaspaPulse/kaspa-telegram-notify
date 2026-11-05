CREATE TABLE IF NOT EXISTS wallet_seen_utxos (
    wallet TEXT NOT NULL,
    outpoint TEXT NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (wallet, outpoint)
);

CREATE INDEX IF NOT EXISTS idx_wallet_seen_utxos_wallet
ON wallet_seen_utxos (wallet);

CREATE TABLE IF NOT EXISTS wallet_alert_dedup (
    wallet TEXT NOT NULL,
    alert_key TEXT NOT NULL,
    txid_masked TEXT NULL,
    block_hash_masked TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (wallet, alert_key)
);

CREATE INDEX IF NOT EXISTS idx_wallet_alert_dedup_created_at
ON wallet_alert_dedup (created_at DESC);

CREATE INDEX IF NOT EXISTS idx_mined_blocks_wallet_timestamp
ON mined_blocks (wallet, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_user_wallets_chat_id
ON user_wallets (chat_id);

CREATE INDEX IF NOT EXISTS idx_user_wallets_wallet
ON user_wallets (wallet);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_severity_created_at
ON bot_event_log (severity, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_bot_event_log_wallet_created_at
ON bot_event_log (wallet_masked, created_at DESC);
