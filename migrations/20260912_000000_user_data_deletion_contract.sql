-- User-data deletion contract support, after all legacy tables exist.
-- Establish runtime privileges here so fresh installs and upgrades do not depend
-- on manual smoke-fixture grants or CI-only role preparation.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kaspa_pulse_app') THEN
        GRANT SELECT, DELETE ON TABLE
            user_wallets,
            telegram_delivery_queue,
            bot_event_log,
            wallet_seen_utxos,
            wallet_alert_dedup,
            pending_rewards,
            mined_blocks
        TO kaspa_pulse_app;
        GRANT SELECT, DELETE ON TABLE chat_history TO kaspa_pulse_app;

        -- Retain security audit history; F01 only removes direct identity links.
        -- Preserve pre-existing grants and add no other audit mutation privilege.
        GRANT SELECT ON TABLE admin_audit_log TO kaspa_pulse_app;
        GRANT UPDATE (admin_actor_user_id, admin_chat_id)
            ON TABLE admin_audit_log TO kaspa_pulse_app;
    END IF;
END
$$;
CREATE INDEX IF NOT EXISTS idx_chat_history_chat_id ON chat_history (chat_id);
