-- Runtime event persistence must work after the documented migration workflow.
-- INSERT allocates the BIGSERIAL id; no table UPDATE or sequence UPDATE is needed.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kaspa_pulse_app') THEN
        GRANT INSERT ON TABLE public.bot_event_log TO kaspa_pulse_app;
        GRANT USAGE ON SEQUENCE public.bot_event_log_id_seq TO kaspa_pulse_app;
    END IF;
END
$$;
