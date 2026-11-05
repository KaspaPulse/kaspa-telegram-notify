-- =============================================================================
-- Kaspa Pulse migration 0004
-- Purpose:
--   Admin audit log and future Telegram delivery queue table.
-- =============================================================================

CREATE TABLE IF NOT EXISTS admin_audit_log (
    id BIGSERIAL PRIMARY KEY,
    admin_chat_id BIGINT NOT NULL,
    action TEXT NOT NULL,
    old_value TEXT,
    new_value TEXT,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_admin_audit_log_created_at
    ON admin_audit_log (created_at DESC);

CREATE INDEX IF NOT EXISTS idx_admin_audit_log_action_created_at
    ON admin_audit_log (action, created_at DESC);

CREATE TABLE IF NOT EXISTS telegram_delivery_queue (
    id BIGSERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL,
    message_html TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_telegram_delivery_queue_status_created
    ON telegram_delivery_queue (status, created_at);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'ck_telegram_delivery_queue_status'
    ) THEN
        ALTER TABLE telegram_delivery_queue
        ADD CONSTRAINT ck_telegram_delivery_queue_status
        CHECK (status IN ('pending', 'sent', 'failed', 'suppressed'));
    END IF;
END $$;

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE admin_audit_log TO kaspa_pulse_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE telegram_delivery_queue TO kaspa_pulse_app;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO kaspa_pulse_app;
