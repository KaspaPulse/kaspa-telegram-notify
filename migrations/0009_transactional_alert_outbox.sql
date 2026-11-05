-- =============================================================================
-- Kaspa Pulse migration 0009
-- Purpose:
--   Make mining-alert persistence transactional and idempotent.
--
-- The queue row is the durable outbox. Each recipient/event pair receives a
-- stable event_key so retries cannot create duplicate pending deliveries.
-- =============================================================================

ALTER TABLE telegram_delivery_queue
    ADD COLUMN IF NOT EXISTS event_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS ux_telegram_delivery_queue_chat_event
    ON telegram_delivery_queue (chat_id, event_key)
    WHERE event_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_event_key
    ON telegram_delivery_queue (event_key)
    WHERE event_key IS NOT NULL;

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE telegram_delivery_queue TO kaspa_pulse_app;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO kaspa_pulse_app;
