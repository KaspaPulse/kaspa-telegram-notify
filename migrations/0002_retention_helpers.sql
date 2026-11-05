-- =============================================================================
-- Kaspa Pulse migration 0002
-- Purpose:
--   Retention and safe cleanup helpers.
-- Notes:
--   Application code may call equivalent cleanup functions, but this function gives
--   operators a safe DB-side cleanup path as well.
-- =============================================================================

CREATE OR REPLACE FUNCTION kaspa_pulse_purge_old_rows(
    event_days INTEGER DEFAULT 30,
    dedup_days INTEGER DEFAULT 30,
    seen_days INTEGER DEFAULT 30
)
RETURNS TABLE (
    purged_bot_events BIGINT,
    purged_wallet_alert_dedup BIGINT,
    purged_wallet_seen_utxos BIGINT
)
LANGUAGE plpgsql
AS $$
DECLARE
    deleted_events BIGINT := 0;
    deleted_dedup BIGINT := 0;
    deleted_seen BIGINT := 0;
BEGIN
    event_days := LEAST(GREATEST(event_days, 1), 365);
    dedup_days := LEAST(GREATEST(dedup_days, 1), 365);
    seen_days := LEAST(GREATEST(seen_days, 1), 365);

    DELETE FROM bot_event_log
    WHERE created_at < NOW() - (event_days::TEXT || ' days')::INTERVAL;
    GET DIAGNOSTICS deleted_events = ROW_COUNT;

    DELETE FROM wallet_alert_dedup
    WHERE created_at < NOW() - (dedup_days::TEXT || ' days')::INTERVAL;
    GET DIAGNOSTICS deleted_dedup = ROW_COUNT;

    DELETE FROM wallet_seen_utxos
    WHERE last_seen_at < NOW() - (seen_days::TEXT || ' days')::INTERVAL;
    GET DIAGNOSTICS deleted_seen = ROW_COUNT;

    RETURN QUERY SELECT deleted_events, deleted_dedup, deleted_seen;
END;
$$;
