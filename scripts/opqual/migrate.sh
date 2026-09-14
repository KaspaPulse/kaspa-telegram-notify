#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase provision
record_op MIGRATE PLANNED "create least-privilege runtime/observer roles and apply repository migrations atomically with harness receipts"

if is_dry_run; then
  info "DRY-RUN: migrations source=$REPO_ROOT/migrations runtime_role=$RUNTIME_ROLE"
  exit 0
fi
assert_owned_container "$POSTGRES_CONTAINER"

# Roles are harness infrastructure only; application schema comes exclusively from repository migrations.
pg_admin <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='$RUNTIME_ROLE') THEN
    CREATE ROLE $RUNTIME_ROLE LOGIN;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='$OBSERVER_ROLE') THEN
    CREATE ROLE $OBSERVER_ROLE LOGIN;
  END IF;
END
\$\$;
GRANT CONNECT ON DATABASE $DATABASE TO $RUNTIME_ROLE, $OBSERVER_ROLE;
GRANT pg_monitor TO $OBSERVER_ROLE;
CREATE TABLE IF NOT EXISTS public.opqual_harness_migrations (
  name text PRIMARY KEY, sha256 text NOT NULL, applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
SQL
MIGRATION_LIST="$RUN_DIR/migration-files.txt"
find "$REPO_ROOT/migrations" -maxdepth 1 -type f -name '*.sql' -print | LC_ALL=C sort > "$MIGRATION_LIST"
[[ -s "$MIGRATION_LIST" ]] || die "no repository migrations found"
: > "$RUN_DIR/migration-receipt.tsv"

while IFS= read -r migration; do
  name="$(basename "$migration")"
  sha="$(sha256_file "$migration")"
  recorded="$(printf "SELECT sha256 FROM public.opqual_harness_migrations WHERE name = :'name';\n" | pg_admin -At -v name="$name" 2>/dev/null || true)"
  if [[ -n "$recorded" ]]; then
    [[ "$recorded" == "$sha" ]] || die "migration receipt hash mismatch for $name"
    printf '%s\t%s\tSKIPPED_ALREADY_VERIFIED\n' "$name" "$sha" >> "$RUN_DIR/migration-receipt.tsv"
    continue
  fi
  record_op "MIGRATION:$name" PLANNED "$sha"
  {
    printf 'BEGIN;\n'
    cat "$migration"
    printf '\nINSERT INTO public.opqual_harness_migrations(name,sha256) VALUES (:\047mname\047,:\047msha\047);\nCOMMIT;\n'
  } | pg_admin -v mname="$name" -v msha="$sha" >/dev/null
  recorded="$(printf "SELECT sha256 FROM public.opqual_harness_migrations WHERE name = :'name';\n" | pg_admin -At -v name="$name")"
  [[ "$recorded" == "$sha" ]] || die "migration receipt verification failed for $name"
  printf '%s\t%s\tAPPLIED\n' "$name" "$sha" >> "$RUN_DIR/migration-receipt.tsv"
  record_op "MIGRATION:$name" VERIFIED "$sha"
done < "$MIGRATION_LIST"
SCHEMA_CHECK="$RUN_DIR/schema-verification.txt"
pg_admin -At > "$SCHEMA_CHECK" <<SQL
SELECT 'database='||current_database();
SELECT 'postgres_major='||current_setting('server_version_num')::int/10000;
SELECT 'runtime_role='||EXISTS(SELECT 1 FROM pg_roles WHERE rolname='$RUNTIME_ROLE');
SELECT 'observer_role='||EXISTS(SELECT 1 FROM pg_roles WHERE rolname='$OBSERVER_ROLE');
SELECT 'user_wallets='||(to_regclass('public.user_wallets') IS NOT NULL);
SELECT 'telegram_delivery_queue='||(to_regclass('public.telegram_delivery_queue') IS NOT NULL);
SELECT 'system_settings='||(to_regclass('public.system_settings') IS NOT NULL);
SELECT 'bot_event_log='||(to_regclass('public.bot_event_log') IS NOT NULL);
SELECT 'queue_select='||has_table_privilege('$RUNTIME_ROLE','public.telegram_delivery_queue','SELECT');
SELECT 'queue_update='||has_table_privilege('$RUNTIME_ROLE','public.telegram_delivery_queue','UPDATE');
SELECT 'wallet_select='||has_table_privilege('$RUNTIME_ROLE','public.user_wallets','SELECT');
SQL

grep -qx "database=$DATABASE" "$SCHEMA_CHECK"
grep -qx 'postgres_major=18' "$SCHEMA_CHECK"
grep -qx 'runtime_role=true' "$SCHEMA_CHECK"
grep -qx 'user_wallets=true' "$SCHEMA_CHECK"
grep -qx 'telegram_delivery_queue=true' "$SCHEMA_CHECK"
grep -qx 'queue_select=true' "$SCHEMA_CHECK"

python3 - <<'PY'
import hashlib,json,os
files=open(os.path.join(os.environ['RUN_DIR'],'migration-files.txt')).read().splitlines()
items=[]
for p in files:
 h=hashlib.sha256(open(p,'rb').read()).hexdigest(); items.append({'path':os.path.relpath(p,os.environ['REPO_ROOT']),'sha256':h})
canon='\n'.join(f"{x['sha256']}  {x['path']}" for x in items).encode()
out={'source':'repository:migrations/','files':items,'set_sha256':hashlib.sha256(canon).hexdigest(),'duplicated_sql':False}
with open(os.path.join(os.environ['RUN_DIR'],'migration-receipt.json'),'w') as f: json.dump(out,f,indent=2,sort_keys=True); f.write('\n')
PY
mark_phase migrate VERIFIED
record_op MIGRATE VERIFIED "repository migrations and privilege/schema contract verified"
info "MIGRATE=PASS"
