-- =============================================================================
-- Kaspa Pulse migration 0003
-- Purpose:
--   Add independent alert delivery toggle.
--   When disabled, detection, DAG analysis, and DB logging continue,
--   but Telegram mining alert delivery is suppressed.
-- =============================================================================

CREATE TABLE IF NOT EXISTS system_settings (
    key_name TEXT PRIMARY KEY,
    value_data TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO system_settings (key_name, value_data)
VALUES ('ENABLE_ALERT_DELIVERY', 'true')
ON CONFLICT (key_name) DO NOTHING;
