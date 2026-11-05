-- =============================================================================
-- Kaspa Pulse migration 0010
-- Purpose:
--   Separate the Telegram actor identity from the destination chat identity
--   in the administrative audit trail.
-- =============================================================================

ALTER TABLE admin_audit_log
    ADD COLUMN IF NOT EXISTS admin_actor_user_id BIGINT;

UPDATE admin_audit_log
SET admin_actor_user_id = admin_chat_id
WHERE admin_actor_user_id IS NULL
  AND admin_chat_id > 0;

CREATE INDEX IF NOT EXISTS idx_admin_audit_actor_created_at
    ON admin_audit_log (admin_actor_user_id, created_at DESC);

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE admin_audit_log TO kaspa_pulse_app;
