-- Runtime data access must be provided by versioned migrations, without CI grants.
-- Restrict writes to the tables/operations used by the production repositories.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kaspa_pulse_app') THEN
        GRANT SELECT, INSERT, UPDATE ON TABLE public.system_settings TO kaspa_pulse_app;
        GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
            public.user_wallets,
            public.wallet_seen_utxos,
            public.pending_rewards
        TO kaspa_pulse_app;
        GRANT SELECT, INSERT, DELETE ON TABLE
            public.mined_blocks,
            public.wallet_alert_dedup
        TO kaspa_pulse_app;
        GRANT USAGE ON SEQUENCE public.mined_blocks_id_seq TO kaspa_pulse_app;
    END IF;
END
$$;
