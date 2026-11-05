-- Stores daily KAS/USD prices used by /blocks.
-- /blocks must read this table only and must not call external market APIs.

CREATE TABLE IF NOT EXISTS kas_price_history (
    day DATE PRIMARY KEY,
    price_usd NUMERIC(20,10) NOT NULL CHECK (price_usd > 0),
    source TEXT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_kas_price_history_fetched_at
    ON kas_price_history (fetched_at DESC);

CREATE INDEX IF NOT EXISTS idx_kas_price_history_updated_at
    ON kas_price_history (updated_at DESC);
-- Grant application roles when they exist. Missing roles are skipped safely.
DO $kas_price_grants$
DECLARE
    app_role TEXT;
BEGIN
    FOREACH app_role IN ARRAY ARRAY[
        'kaspa',
        'kaspa_app',
        'kaspa_dev',
        'kaspa_pulse',
        'kaspa_pulse_app',
        'kaspa_user'
    ]
    LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = app_role) THEN
            EXECUTE format('GRANT SELECT, INSERT, UPDATE ON TABLE kas_price_history TO %I', app_role);
        END IF;
    END LOOP;
END
$kas_price_grants$;
