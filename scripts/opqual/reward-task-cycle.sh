#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase migrate
require_phase fixtures
require_phase start_app
RECEIPT="$RUN_DIR/reward-task-cycle.json"
TXID='1111111111111111111111111111111111111111111111111111111111111111'
OUTPOINT="$TXID:0"
BASELINE_OUTPOINT='opqual-reward-baseline:0'

if [[ -f "$RECEIPT" ]]; then
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); assert d.get("result")=="PASS"' "$RECEIPT"
  info "REWARD_TASK_CYCLE=PASS_REUSED"
  exit 0
fi

record_op REWARD_TASK_CYCLE PLANNED "exercise real utxo_reward_analysis task with controlled non-coinbase synthetic UTXO through local Kaspa wRPC fixture"
assert_owned_container "$APP_CONTAINER"
[[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" == true ]] || die "primary app not running"

# Pause node traffic while DB baseline and fixture control are made consistent.
# Resume is safe if a prior recovery already removed the owned node container.
if container_exists "$NODE_CONTAINER"; then
  assert_owned_container "$NODE_CONTAINER"
  if [[ "$(sudo -n docker inspect "$NODE_CONTAINER" --format '{{.State.Running}}')" == true ]]; then
    sudo -n docker kill --signal=TERM "$NODE_CONTAINER" >/dev/null || true
    for i in $(seq 1 40); do
      [[ "$(sudo -n docker inspect "$NODE_CONTAINER" --format '{{.State.Running}}')" == false ]] && break
      [[ "$i" -lt 40 ]] || die "Kaspa fixture did not stop for controlled restart"
      sleep 0.25
    done
  fi
  safe_rm_container "$NODE_CONTAINER"
fi

pg_admin -v wallet="$SYNTH_WALLET" -v chat="$SYNTH_CHAT_ID" -v baseline="$BASELINE_OUTPOINT" -v outpoint="$OUTPOINT" -v txid="$TXID" <<'SQL'
INSERT INTO user_wallets(wallet,chat_id) VALUES (:'wallet',:chat::bigint)
ON CONFLICT(wallet,chat_id) DO NOTHING;
DELETE FROM telegram_delivery_queue WHERE event_key=:'outpoint';
DELETE FROM wallet_alert_dedup WHERE wallet=:'wallet' AND alert_key=:'txid';
DELETE FROM pending_rewards WHERE wallet=:'wallet' AND outpoint=:'outpoint';
DELETE FROM wallet_seen_utxos WHERE wallet=:'wallet';
INSERT INTO wallet_seen_utxos(wallet,outpoint) VALUES (:'wallet',:'baseline')
ON CONFLICT(wallet,outpoint) DO UPDATE SET last_seen_at=NOW();
SQL

export OPQUAL_REWARD_TXID="$TXID" OPQUAL_REWARD_WALLET="$SYNTH_WALLET"
python3 - <<'PY'
import json,os
p=os.path.join(os.environ['CONTROL_DIR'],'kaspa.json')
entry={
  'address':os.environ['OPQUAL_REWARD_WALLET'],
  'outpoint':{'transactionId':os.environ['OPQUAL_REWARD_TXID'],'index':0},
  'utxoEntry':{'amount':100000000,'scriptPublicKey':'0000','blockDaaScore':1,'isCoinbase':False,'covenantId':None},
}
with open(p+'.tmp','w') as f: json.dump({'utxos':[entry]},f,sort_keys=True); f.write('\n')
os.replace(p+'.tmp',p)
PY
APP_LOG_START="$(wc -l < "$RUN_DIR/app.stdout.log")"
KASPA_EVENT_START="$(wc -l < "$EVENTS_DIR/kaspa-events.jsonl")"

sudo -n docker run -d --name "$NODE_CONTAINER" --network "$DOCKER_NETWORK" --network-alias "$NODE_CONTAINER" \
  --label "task=$TASK_ID" --label purpose=kaspa-wrpc \
  --read-only --security-opt no-new-privileges:true --tmpfs /tmp:rw,noexec,nosuid,size=8m \
  --mount "type=bind,src=$OPQUAL_DIR/fixtures,dst=/fixture,readonly" \
  --mount "type=bind,src=$CONTROL_DIR,dst=/control" \
  --mount "type=bind,src=$EVENTS_DIR,dst=/evidence" \
  --mount "type=bind,src=$CERTS_DIR,dst=/certs,readonly" \
  "$PYTHON_IMAGE" python -B -u /fixture/kaspa_wrpc.py > "$RECEIPTS_DIR/reward-node-restart.id"
assert_owned_container "$NODE_CONTAINER"
assert_no_published_ports "$NODE_CONTAINER"

for i in $(seq 1 160); do
  if tail -n +$((APP_LOG_START+1)) "$RUN_DIR/app.stdout.log" | grep -q '\[TASK START\] utxo_reward_analysis'; then break; fi
  [[ "$i" -lt 160 ]] || {
    tail -n +$((APP_LOG_START+1)) "$RUN_DIR/app.stdout.log" > "$RUN_DIR/reward-task-timeout-app.log"
    tail -n +$((KASPA_EVENT_START+1)) "$EVENTS_DIR/kaspa-events.jsonl" > "$RUN_DIR/reward-task-timeout-node.jsonl"
    die "utxo_reward_analysis task did not start within 40s"
  }
  sleep 0.25
done
for i in $(seq 1 80); do
  if tail -n +$((APP_LOG_START+1)) "$RUN_DIR/app.stdout.log" | grep -q '\[TASK MONITOR\] utxo_reward_analysis joined cleanly'; then break; fi
  [[ "$i" -lt 80 ]] || die "utxo_reward_analysis did not join cleanly"
  sleep 0.25
done

tail -n +$((APP_LOG_START+1)) "$RUN_DIR/app.stdout.log" | \
  grep -E '\[TASK (START|STOP|MONITOR)\] utxo_reward_analysis|DAG ANALYSIS|ALERT OUTBOX' \
  > "$RUN_DIR/reward-task-markers.log" || true
grep -q '\[TASK START\] utxo_reward_analysis' "$RUN_DIR/reward-task-markers.log"
grep -q '\[TASK STOP\] utxo_reward_analysis finished normally' "$RUN_DIR/reward-task-markers.log"
grep -q '\[TASK MONITOR\] utxo_reward_analysis joined cleanly' "$RUN_DIR/reward-task-markers.log"

# Disable the synthetic UTXO before cleaning its durable state.
printf '{}\n' > "$CONTROL_DIR/kaspa.json"
sleep 2
pg_admin -v wallet="$SYNTH_WALLET" -v baseline="$BASELINE_OUTPOINT" -v outpoint="$OUTPOINT" -v txid="$TXID" <<'SQL'
DELETE FROM telegram_delivery_queue WHERE event_key=:'outpoint';
DELETE FROM wallet_alert_dedup WHERE wallet=:'wallet' AND alert_key=:'txid';
DELETE FROM pending_rewards WHERE wallet=:'wallet' AND outpoint=:'outpoint';
DELETE FROM wallet_seen_utxos WHERE wallet=:'wallet' AND outpoint IN (:'baseline',:'outpoint');
SQL

export OPQUAL_REWARD_OUTPOINT="$OUTPOINT"
python3 - <<'PY'
import json,os
markers=open(os.path.join(os.environ['RUN_DIR'],'reward-task-markers.log'),encoding='utf-8').read()
out={
 'result':'PASS',
 'synthetic_utxo':{'outpoint':os.environ['OPQUAL_REWARD_OUTPOINT'],'is_coinbase':False},
 'task_start':'[TASK START] utxo_reward_analysis' in markers,
 'task_stop':'[TASK STOP] utxo_reward_analysis finished normally' in markers,
 'task_join':'[TASK MONITOR] utxo_reward_analysis joined cleanly' in markers,
 'provider':'local Kaspa wRPC fixture',
 'real_user_data':False,
}
with open(os.path.join(os.environ['RUN_DIR'],'reward-task-cycle.json'),'w') as f:
    json.dump(out,f,indent=2,sort_keys=True); f.write('\n')
PY
record_op REWARD_TASK_CYCLE VERIFIED "real application utxo_reward_analysis task started, stopped normally and joined cleanly from controlled non-coinbase synthetic UTXO"
info "REWARD_TASK_CYCLE=PASS"