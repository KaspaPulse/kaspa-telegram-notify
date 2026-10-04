# Database Security

## Production policy

- Do not run the bot with the PostgreSQL `postgres` superuser.
- Runtime `.env` must use the least-privilege role `kaspa_pulse_app`.
- The app role receives only:
  - CONNECT on the target database
  - USAGE on schema `public`
  - SELECT / INSERT / UPDATE / DELETE on application tables
  - USAGE / SELECT / UPDATE on application sequences
  - EXECUTE on the safe retention helper, when available
- Schema changes must be applied through SQL migration files in `migrations/`.
- `ALLOW_RUNTIME_SCHEMA_ENSURE=false` in production.
- Use `sslmode=require` for remote databases.
- Local-only database connections on 127.0.0.1 may use `sslmode=disable`.

## Backup

Run:

    cargo xtask database backup

## Restore

Run:

    cargo xtask database restore backups/db/YOUR_FILE.dump

## Migrations

Run:

    cargo xtask database migrate

## Retention helper

SQL:

    SELECT * FROM kaspa_pulse_purge_old_rows(30, 30, 30);

## Important notes

- Keep `.env` private.
- Do not commit database passwords.
- Do not use the PostgreSQL superuser for runtime.
- Keep backups encrypted or stored in a private location.
- Database dumps, backup archives, restore scratch files, and runtime evidence are local/private operational artifacts and must not be committed to the repository.
