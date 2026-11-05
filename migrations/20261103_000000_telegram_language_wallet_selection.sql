-- OP003: additive, scoped UI persistence for existing Telegram users.
-- Apply only after isolated PostgreSQL verification and cutover approval.
-- No changes to existing wallet or alert rows.
CREATE TABLE IF NOT EXISTS public.telegram_user_language (
    user_id BIGINT PRIMARY KEY,
    language TEXT NOT NULL DEFAULT 'en',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT telegram_user_language_supported CHECK (language IN ('ar', 'en'))
);

CREATE TABLE IF NOT EXISTS public.telegram_active_wallet (
    chat_id BIGINT PRIMARY KEY,
    wallet TEXT NOT NULL,
    selected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT telegram_active_wallet_owned
        FOREIGN KEY (wallet, chat_id)
        REFERENCES public.user_wallets(wallet, chat_id)
        ON DELETE CASCADE
);

GRANT SELECT, INSERT, UPDATE ON TABLE public.telegram_user_language TO kaspa_pulse_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.telegram_active_wallet TO kaspa_pulse_app;
