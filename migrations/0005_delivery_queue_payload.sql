-- =============================================================================
-- Kaspa Pulse migration 0005
-- Purpose:
--   Complete Telegram delivery queue with mining-alert payload metadata.
-- =============================================================================

ALTER TABLE telegram_delivery_queue
    ADD COLUMN IF NOT EXISTS wallet_masked TEXT,
    ADD COLUMN IF NOT EXISTS txid_masked TEXT,
    ADD COLUMN IF NOT EXISTS block_hash_masked TEXT,
    ADD COLUMN IF NOT EXISTS amount_kas DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS daa_score BIGINT;

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_status_attempts_created
    ON telegram_delivery_queue (status, attempts, created_at);

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_wallet_created
    ON telegram_delivery_queue (wallet_masked, created_at DESC);

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE telegram_delivery_queue TO kaspa_pulse_app;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO kaspa_pulse_app;
