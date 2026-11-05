-- =============================================================================
-- Kaspa Pulse migration 0006
-- Purpose:
--   Production hardening for telegram_delivery_queue.
--   Adds processing locks, retry backoff, worker ownership, and queue metrics indexes.
-- =============================================================================

ALTER TABLE telegram_delivery_queue
    ADD COLUMN IF NOT EXISTS locked_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS locked_by TEXT,
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'ck_telegram_delivery_queue_status_v2'
    ) THEN
        ALTER TABLE telegram_delivery_queue
        ADD CONSTRAINT ck_telegram_delivery_queue_status_v2
        CHECK (status IN ('pending', 'processing', 'sent', 'failed', 'suppressed'));
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_ready
    ON telegram_delivery_queue (status, next_attempt_at, created_at)
    WHERE status IN ('pending', 'processing');

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_locked
    ON telegram_delivery_queue (locked_at)
    WHERE status = 'processing';

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE telegram_delivery_queue TO kaspa_pulse_app;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO kaspa_pulse_app;
