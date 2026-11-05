-- =============================================================================
-- Kaspa Pulse migration: legacy user_wallets subscription-incarnation timestamp
-- Purpose:
--   Upgrade legacy Production schemas where user_wallets predates created_at.
--   Delivery claim authorization uses this timestamp to prevent an old queued
--   alert from being revived by a later delete/re-subscribe cycle.
--
-- Backfill policy:
--   last_active is an upper bound on the unknown historical creation time.
--   Using it is fail-closed: it may suppress an old queued delivery, but it
--   never makes an older queue event appear newer than a known subscription.
-- =============================================================================

ALTER TABLE user_wallets
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ;

UPDATE user_wallets
SET created_at = COALESCE(last_active, CURRENT_TIMESTAMP)
WHERE created_at IS NULL;

ALTER TABLE user_wallets
    ALTER COLUMN created_at SET DEFAULT NOW();

ALTER TABLE user_wallets
    ALTER COLUMN created_at SET NOT NULL;
