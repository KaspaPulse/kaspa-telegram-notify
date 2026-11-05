-- =============================================================================
-- Kaspa Pulse migration 0007
-- Purpose:
--   Fix telegram_delivery_queue status constraint after processing locks.
--
-- Background:
--   Migration 0006 introduced status = 'processing' for safe queue locking,
--   but production databases can still have the legacy 0004 constraint:
--   ck_telegram_delivery_queue_status
--   which only allows: pending, sent, failed, suppressed.
--
-- This migration removes the legacy constraint and ensures the v2 constraint
-- allows: pending, processing, sent, failed, suppressed.
-- =============================================================================

ALTER TABLE telegram_delivery_queue
    ADD COLUMN IF NOT EXISTS locked_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS locked_by TEXT,
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

ALTER TABLE telegram_delivery_queue
    DROP CONSTRAINT IF EXISTS ck_telegram_delivery_queue_status;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'telegram_delivery_queue'::regclass
          AND conname = 'ck_telegram_delivery_queue_status_v2'
    ) THEN
        ALTER TABLE telegram_delivery_queue
        ADD CONSTRAINT ck_telegram_delivery_queue_status_v2
        CHECK (status IN ('pending', 'processing', 'sent', 'failed', 'suppressed')) NOT VALID;
    END IF;
END $$;

ALTER TABLE telegram_delivery_queue
    VALIDATE CONSTRAINT ck_telegram_delivery_queue_status_v2;

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_ready
    ON telegram_delivery_queue (status, next_attempt_at, created_at)
    WHERE status IN ('pending', 'processing');

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_locked
    ON telegram_delivery_queue (locked_at)
    WHERE status = 'processing';

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE telegram_delivery_queue TO kaspa_pulse_app;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO kaspa_pulse_app;
