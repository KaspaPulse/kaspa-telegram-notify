#!/usr/bin/env bash
set -Eeuo pipefail

: "${DATABASE_ADMIN_URL:?DATABASE_ADMIN_URL is required}"
: "${DATABASE_URL:?DATABASE_URL is required}"

echo "Waiting for PostgreSQL..."
for attempt in $(seq 1 30); do
    if pg_isready \
        --dbname="$DATABASE_ADMIN_URL" \
        --quiet
    then
        break
    fi

    if [[ "$attempt" -eq 30 ]]; then
        echo "PostgreSQL did not become ready." >&2
        exit 1
    fi

    sleep 1
done

echo "Creating or refreshing the CI runtime role..."
psql "$DATABASE_ADMIN_URL" \
    -v ON_ERROR_STOP=1 <<'SQL'
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_roles
        WHERE rolname = 'kaspa_pulse_app'
    ) THEN
        CREATE ROLE kaspa_pulse_app
            LOGIN
            PASSWORD 'TEST_PASSWORD';
    ELSE
        ALTER ROLE kaspa_pulse_app
            WITH LOGIN
            PASSWORD 'TEST_PASSWORD';
    END IF;
END
$$;
SQL

echo "Applying migrations in lexical order..."
while IFS= read -r migration; do
    echo "Applying migration: ${migration}"
    psql "$DATABASE_ADMIN_URL" \
        -v ON_ERROR_STOP=1 \
        -f "$migration"
done < <(
    find migrations \
        -maxdepth 1 \
        -type f \
        -name '*.sql' \
        -print |
    sort
)

echo "Granting CI-only reset privileges; runtime DML comes from migrations..."
psql "$DATABASE_ADMIN_URL" \
    -v ON_ERROR_STOP=1 <<'SQL'
GRANT TRUNCATE ON TABLE
    telegram_delivery_queue,
    wallet_alert_dedup,
    user_wallets
TO kaspa_pulse_app;

SQL

echo "Verifying the complete CI runtime privilege matrix..."
psql "$DATABASE_URL" \
    -v ON_ERROR_STOP=1 \
    -Atqc "
        SELECT
            to_regclass('public.telegram_delivery_queue') IS NOT NULL
            AND to_regclass('public.wallet_alert_dedup') IS NOT NULL
            AND to_regclass('public.user_wallets') IS NOT NULL
            AND to_regclass('public.wallet_seen_utxos') IS NOT NULL
            AND to_regclass('public.system_settings') IS NOT NULL
            AND to_regclass('public.telegram_delivery_queue_id_seq') IS NOT NULL

            AND has_table_privilege(
                current_user,
                'public.telegram_delivery_queue',
                'SELECT'
            )
            AND has_table_privilege(
                current_user,
                'public.telegram_delivery_queue',
                'INSERT'
            )
            AND has_table_privilege(
                current_user,
                'public.telegram_delivery_queue',
                'UPDATE'
            )
            AND has_table_privilege(
                current_user,
                'public.telegram_delivery_queue',
                'DELETE'
            )
            AND has_table_privilege(
                current_user,
                'public.telegram_delivery_queue',
                'TRUNCATE'
            )
            AND has_sequence_privilege(
                current_user,
                'public.telegram_delivery_queue_id_seq',
                'USAGE'
            )

            AND has_table_privilege(
                current_user,
                'public.wallet_alert_dedup',
                'SELECT'
            )
            AND has_table_privilege(
                current_user,
                'public.wallet_alert_dedup',
                'INSERT'
            )
            AND has_table_privilege(
                current_user,
                'public.wallet_alert_dedup',
                'DELETE'
            )
            AND has_table_privilege(
                current_user,
                'public.wallet_alert_dedup',
                'TRUNCATE'
            )

            AND has_table_privilege(
                current_user,
                'public.user_wallets',
                'SELECT'
            )
            AND has_table_privilege(
                current_user,
                'public.user_wallets',
                'INSERT'
            )
            AND has_table_privilege(
                current_user,
                'public.user_wallets',
                'UPDATE'
            )
            AND has_table_privilege(
                current_user,
                'public.user_wallets',
                'DELETE'
            )
            AND has_table_privilege(
                current_user,
                'public.user_wallets',
                'TRUNCATE'
            )

            AND has_table_privilege(
                current_user,
                'public.wallet_seen_utxos',
                'SELECT'
            )
            AND has_table_privilege(
                current_user,
                'public.wallet_seen_utxos',
                'INSERT'
            )
            AND has_table_privilege(
                current_user,
                'public.wallet_seen_utxos',
                'UPDATE'
            )
            AND has_table_privilege(
                current_user,
                'public.wallet_seen_utxos',
                'DELETE'
            )

            AND has_table_privilege(
                current_user,
                'public.system_settings',
                'SELECT'
            )
            AND has_table_privilege(
                current_user,
                'public.system_settings',
                'INSERT'
            )
            AND has_table_privilege(
                current_user,
                'public.system_settings',
                'UPDATE'
            );
    " |
grep -qx 't'

echo "CI PostgreSQL schema is ready."
