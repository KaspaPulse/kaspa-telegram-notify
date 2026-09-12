-- User-data deletion contract support.
-- Legacy chat_history remains for backward compatibility but is now explicitly
-- covered by the runtime deletion transaction.
DO $$
BEGIN
    IF to_regclass('public.chat_history') IS NOT NULL
       AND EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kaspa_pulse_app') THEN
        GRANT SELECT, DELETE ON TABLE chat_history TO kaspa_pulse_app;
    END IF;
END
$$;
CREATE INDEX IF NOT EXISTS idx_chat_history_chat_id ON chat_history (chat_id);
