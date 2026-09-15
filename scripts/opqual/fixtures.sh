#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase migrate
record_op FIXTURES PLANNED "create task CA, synthetic Telegram/Kaspa/HTTP providers, and synthetic DB identities"

if is_dry_run; then
  info "DRY-RUN: would create only synthetic local fixtures on $DOCKER_NETWORK"
  exit 0
fi
assert_internal_network

if [[ ! -f "$CERTS_DIR/ca.crt" ]]; then
  openssl genrsa -out "$CERTS_DIR/ca.key" 2048 >/dev/null 2>&1
  openssl req -x509 -new -sha256 -days 2 -key "$CERTS_DIR/ca.key" \
    -subj '/CN=Kaspa Pulse OpQual Synthetic CA' \
    -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' \
    -out "$CERTS_DIR/ca.crt" >/dev/null 2>&1
  openssl genrsa -out "$CERTS_DIR/server.key" 2048 >/dev/null 2>&1
  openssl req -new -key "$CERTS_DIR/server.key" -subj '/CN=api.telegram.org' -out "$CERTS_DIR/server.csr" >/dev/null 2>&1
  cat > "$CERTS_DIR/server.ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:api.telegram.org,DNS:api.kaspa.org,DNS:api.coingecko.com
EOF
  openssl x509 -req -sha256 -days 2 -in "$CERTS_DIR/server.csr" -CA "$CERTS_DIR/ca.crt" \
    -CAkey "$CERTS_DIR/ca.key" -CAcreateserial -extfile "$CERTS_DIR/server.ext" -out "$CERTS_DIR/server.crt" >/dev/null 2>&1
fi
printf '{}\n' > "$CONTROL_DIR/telegram.json"
printf '{}\n' > "$CONTROL_DIR/http.json"
printf '{}\n' > "$CONTROL_DIR/kaspa.json"
: > "$CONTROL_DIR/telegram-updates.jsonl"

start_fixture() {
  local name="$1" purpose="$2" script="$3"; shift 3
  if ! container_exists "$name"; then
    sudo -n docker run -d --name "$name" --network "$DOCKER_NETWORK" "$@" \
      --label "task=$TASK_ID" --label "purpose=$purpose" \
      --read-only --security-opt no-new-privileges:true --tmpfs /tmp:rw,noexec,nosuid,size=8m \
      --mount "type=bind,src=$OPQUAL_DIR/fixtures,dst=/fixture,readonly" \
      --mount "type=bind,src=$CONTROL_DIR,dst=/control" \
      --mount "type=bind,src=$EVENTS_DIR,dst=/evidence" \
      --mount "type=bind,src=$CERTS_DIR,dst=/certs,readonly" \
      "$PYTHON_IMAGE" python -B -u "/fixture/$script" > "$RECEIPTS_DIR/$name.id"
  else
    assert_owned_container "$name"
    [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == true ]] || sudo -n docker start "$name" >/dev/null
  fi
  assert_no_published_ports "$name"
}

start_fixture "$TELEGRAM_CONTAINER" telegram telegram_bot_api.py --network-alias api.telegram.org
start_fixture "$HTTP_CONTAINER" http-providers http_providers.py --network-alias api.kaspa.org --network-alias api.coingecko.com
start_fixture "$NODE_CONTAINER" kaspa-wrpc kaspa_wrpc.py --network-alias "$NODE_CONTAINER"
for name in "$TELEGRAM_CONTAINER" "$HTTP_CONTAINER" "$NODE_CONTAINER"; do
  for i in $(seq 1 40); do
    [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == true ]] && break
    [[ "$i" -lt 40 ]] || die "fixture failed to stay running: $name"
    sleep 0.25
  done
done

sudo -n docker run --rm -i --network "$DOCKER_NETWORK" \
  --mount "type=bind,src=$CERTS_DIR/ca.crt,dst=/ca.crt,readonly" "$PYTHON_IMAGE" python - <<'PY' \
  > "$RUN_DIR/provider-self-check.txt"
import json,ssl,urllib.request
ctx=ssl.create_default_context(cafile='/ca.crt')
urls=['https://api.telegram.org/bot1234567890:TEST_TOKEN_NOT_REAL/getMe',
      'https://api.kaspa.org/info/price','https://api.kaspa.org/info/marketcap',
      'https://api.kaspa.org/info/fee-estimate',
      'https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd&include_market_cap=true']
for url in urls:
    with urllib.request.urlopen(url,context=ctx,timeout=5) as r:
        print(r.status,url,json.loads(r.read()))
PY
[[ "$(grep -c '^200 ' "$RUN_DIR/provider-self-check.txt")" -eq 5 ]] || die "provider self-check failed"

sudo -n docker run --rm -i --network "$DOCKER_NETWORK" "$PYTHON_IMAGE" python - <<PY \
  > "$RUN_DIR/node-self-check.txt"
import urllib.request
print(urllib.request.urlopen('http://$NODE_CONTAINER:$NODE_PORT/',timeout=5).read().decode().strip())
PY
grep -qx 'local kaspa fixture' "$RUN_DIR/node-self-check.txt" || die "Kaspa fixture self-check failed"
pg_admin -v chat="$SYNTH_CHAT_ID" -v wallet="$SYNTH_WALLET" <<'SQL'
INSERT INTO system_settings(key_name,value_data) VALUES
 ('ENABLE_MEMORY_CLEANER','false'),('ENABLE_LIVE_SYNC','true'),('MAINTENANCE_MODE','false')
ON CONFLICT(key_name) DO UPDATE SET value_data=EXCLUDED.value_data;
INSERT INTO user_wallets(wallet,chat_id) VALUES (:'wallet', :chat::bigint)
ON CONFLICT(wallet,chat_id) DO NOTHING;
INSERT INTO kas_price_history(day,price_usd,source)
SELECT d::date,0.123456,'opqual-synthetic'
FROM generate_series(CURRENT_DATE-100,CURRENT_DATE,interval '1 day') d
ON CONFLICT(day) DO NOTHING;
INSERT INTO bot_event_log(event_type,severity,chat_id,status,metadata)
VALUES ('SYSTEM_START','info',:chat::bigint,'synthetic_seed','{"opqual":true}'::jsonb);
SQL

python3 - <<'PY'
import hashlib,json,os
root=os.environ['OPQUAL_DIR']; out=os.environ['RUN_DIR']
files=['fixtures/telegram_bot_api.py','fixtures/http_providers.py','fixtures/kaspa_wrpc.py']
hashes={p:hashlib.sha256(open(os.path.join(root,p),'rb').read()).hexdigest() for p in files}
with open(os.path.join(out,'fixture-hashes.json'),'w') as f: json.dump(hashes,f,indent=2,sort_keys=True); f.write('\n')
ident={'admin_user_id':int(os.environ['SYNTH_ADMIN_ID']),'ordinary_user_id':int(os.environ['SYNTH_USER_ID']),
       'ordinary_chat_id':int(os.environ['SYNTH_CHAT_ID']),'wallet':os.environ['SYNTH_WALLET'],
       'real_identity':False,'source':'deterministic synthetic test identity'}
with open(os.path.join(out,'synthetic-identities.json'),'w') as f: json.dump(ident,f,indent=2,sort_keys=True); f.write('\n')
PY
capture_docker_inventory
mark_phase fixtures VERIFIED
record_op FIXTURES VERIFIED "synthetic-only providers and DB fixtures ready; no public listeners"
info "FIXTURES=PASS"
