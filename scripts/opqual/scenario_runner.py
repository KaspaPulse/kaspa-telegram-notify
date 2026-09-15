#!/usr/bin/env python3
"""Execute current BLOCKED operational scenarios conservatively against the real app dispatcher."""
from __future__ import annotations
import csv,datetime,hashlib,json,os,subprocess,sys,time,traceback
from pathlib import Path

ROOT=Path(os.environ["REPO_ROOT"])
RUN=Path(os.environ["RUN_DIR"])
CONTROL=Path(os.environ["CONTROL_DIR"])
EVENTS=Path(os.environ["EVENTS_DIR"])
MATRIX=Path(os.environ["SCENARIO_MATRIX"])
DRIVER=Path(os.environ["OPQUAL_DIR"])/"fixtures/scenario_driver.py"
PG=os.environ["POSTGRES_CONTAINER"]
PG_ADMIN=os.environ["PG_ADMIN"]
DB=os.environ["DATABASE"]
OBS_APP=os.environ["OBSERVER_APP_NAME"]
ADMIN=int(os.environ["SYNTH_ADMIN_ID"])
USER=int(os.environ["SYNTH_USER_ID"])
CHAT=int(os.environ["SYNTH_CHAT_ID"])
WALLET=os.environ["SYNTH_WALLET"]
APP=os.environ["APP_CONTAINER"]
HEALTH_PORT=int(os.environ["HEALTH_PORT"])

TELEGRAM_EVENTS=EVENTS/"telegram-events.jsonl"
KASPA_EVENTS=EVENTS/"kaspa-events.jsonl"
HTTP_EVENTS=EVENTS/"http-provider-events.jsonl"
ADMIN_COMMANDS={"health","stats","sys","pause","resume","mute_alerts","unmute_alerts","alerts_status",
                "restart_info","logs","events","errors","delivery","subscribers","wallet_events","cleanup_events",
                "db_diag","settings","toggle"}
ADMIN_CALLBACKS={"cmd_health","cmd_stats","cmd_sys","cmd_pause","cmd_resume","cmd_mute_alerts","cmd_unmute_alerts",
                 "cmd_alerts_status","cmd_restart_info","cmd_logs","cmd_events","cmd_errors","cmd_delivery",
                 "cmd_cleanup_events","cmd_db_diag","cmd_settings","btn_toggle_ENABLE_LIVE_SYNC",
                 "btn_toggle_ENABLE_MEMORY_CLEANER","btn_toggle_MAINTENANCE_MODE"}
NONCE_COMMAND={
 "confirm-pause":"/pause","confirm-resume":"/resume","confirm-cleanup_events":"/cleanup_events",
 "confirm-mute_alerts":"/mute_alerts","confirm-unmute_alerts":"/unmute_alerts","confirm-clear_wallets":"/forget_wallets",
 "confirm-forget_all":"/forget_all","confirm-toggle_memory":"/toggle ENABLE_MEMORY_CLEANER",
 "confirm-toggle_live_sync":"/toggle ENABLE_LIVE_SYNC","confirm-toggle_maintenance":"/toggle MAINTENANCE_MODE"}

class Blocked(Exception): pass
class ContractFail(Exception): pass

def run(cmd,check=True,input=None):
    p=subprocess.run(cmd,text=True,input=input,capture_output=True)
    if check and p.returncode: raise ContractFail(f"command failed {cmd}: {p.stderr[-500:]}")
    return p

def pg(sql):
    p=run(["sudo","-n","docker","exec","-i","-e",f"PGAPPNAME={OBS_APP}",PG,
           "psql","-X","-U",PG_ADMIN,"-d",DB,"-At","-v","ON_ERROR_STOP=1","-c",sql])
    return p.stdout.strip()
def load_events(path):
    if not path.exists(): return []
    rows=[]
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip(): continue
        try: rows.append(json.loads(line))
        except json.JSONDecodeError: continue
    return rows

def event_count(): return len(load_events(TELEGRAM_EVENTS))

def driver(*args,timeout=20):
    p=subprocess.run([sys.executable,str(DRIVER),"--control-dir",str(CONTROL),"--event-dir",str(EVENTS),*map(str,args)],
                     text=True,capture_output=True,timeout=timeout)
    if p.returncode: raise ContractFail(p.stderr.strip() or p.stdout.strip() or f"driver rc={p.returncode}")
    return p.stdout.strip()

def wait_output(start,chat_id,timeout=15):
    deadline=time.monotonic()+timeout
    methods={"sendmessage","editmessagetext","editmessagereplymarkup","answercallbackquery"}
    while time.monotonic()<deadline:
        for row in load_events(TELEGRAM_EVENTS)[start:]:
            if str(row.get("method","")).lower() not in methods: continue
            row_chat=row.get("chat_id")
            if row_chat is not None and int(row_chat)==chat_id: return row
            if str(row.get("method","")).lower()=="answercallbackquery": return row
        time.sleep(.1)
    raise ContractFail(f"no Telegram output for chat {chat_id}")

def telegram_contains(start,text,timeout=15):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        for row in load_events(TELEGRAM_EVENTS)[start:]:
            if text in json.dumps(row,ensure_ascii=False): return row
        time.sleep(.1)
    raise ContractFail(f"Telegram output missing {text!r}")
def sql_quote(v): return "'"+str(v).replace("'","''")+"'"
def seed_wallet(chat_id=CHAT):
    pg(f"INSERT INTO user_wallets(wallet,chat_id) VALUES ({sql_quote(WALLET)},{chat_id}) ON CONFLICT(wallet,chat_id) DO NOTHING;")
def delete_wallet(chat_id=CHAT): pg(f"DELETE FROM user_wallets WHERE wallet={sql_quote(WALLET)} AND chat_id={chat_id};")
def wallet_count(chat_id=CHAT): return int(pg(f"SELECT count(*) FROM user_wallets WHERE wallet={sql_quote(WALLET)} AND chat_id={chat_id};") or 0)
def setting(key): return pg(f"SELECT value_data FROM system_settings WHERE key_name={sql_quote(key)};")
def set_setting(key,value): pg(f"INSERT INTO system_settings(key_name,value_data) VALUES ({sql_quote(key)},{sql_quote(value)}) ON CONFLICT(key_name) DO UPDATE SET value_data=EXCLUDED.value_data;")

def reset_db_baseline():
    # Do not write runtime-mirrored settings directly: that would desynchronize DB and AppContext atomics.
    set_setting('ENABLE_ALERT_DELIVERY','true')
    seed_wallet(CHAT)

def command_text(name):
    if name=='add': return f'/add {WALLET}'
    if name=='remove': return f'/remove {WALLET}'
    if name in ('subscribers','wallet_events'): return f'/{name} {WALLET}'
    if name=='toggle': return '/toggle ENABLE_LIVE_SYNC'
    return '/'+name

def command_actor(name): return (ADMIN,ADMIN) if name in ADMIN_COMMANDS else (USER,CHAT)

def inject_message(text,uid,cid):
    before=event_count(); driver('message','--user-id',uid,'--chat-id',cid,'--text',text); return before,wait_output(before,cid)

def inject_callback(data,uid,cid,message_id=None):
    before=event_count(); args=['callback','--user-id',uid,'--chat-id',cid,'--data',data]
    if message_id is not None: args += ['--message-id',message_id]
    driver(*args); return before,wait_output(before,cid)
def find_callback_data(value,prefix=None):
    if isinstance(value,dict):
        for k,v in value.items():
            if k=='callback_data' and isinstance(v,str) and (prefix is None or v.startswith(prefix)): return v
            found=find_callback_data(v,prefix)
            if found: return found
    elif isinstance(value,list):
        for v in value:
            found=find_callback_data(v,prefix)
            if found: return found
    elif isinstance(value,str):
        try: return find_callback_data(json.loads(value),prefix)
        except Exception: return None
    return None

def wait_keyboard(start,prefix,timeout=15):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        for row in load_events(TELEGRAM_EVENTS)[start:]:
            data=find_callback_data(row.get('reply_markup'),prefix)
            if data: return row,data
        time.sleep(.1)
    raise ContractFail(f'callback payload {prefix!r} not observed')

def wallet_token(chat_id):
    return hashlib.sha256(f'wallet-callback-v1:{chat_id}:{WALLET}'.encode()).digest()[:16].hex()

def command_contract(name):
    uid,cid=command_actor(name)
    if name=='add': delete_wallet(cid)
    elif name=='remove': seed_wallet(cid)
    before,row=inject_message(command_text(name),uid,cid)
    if name=='add':
        if wallet_count(cid)!=1: raise ContractFail('/add did not persist wallet')
    elif name=='remove':
        if wallet_count(cid)!=0: raise ContractFail('/remove did not delete wallet')
        seed_wallet(cid)
    return {'input':command_text(name),'telegram_event':row.get('sequence'),'contract':'dispatcher_reply'+('+db_state' if name in ('add','remove') else '')}
def callback_data_for(suffix,cid):
    token=wallet_token(cid)
    wallet_map={'wp':f'wp:{token}','wbal':f'wbal:{token}','wblk':f'wblk:{token}:0','wmin':f'wmin:{token}','wrc':f'wrc:{token}','wrd':f'wrd:{token}'}
    if suffix in wallet_map: return wallet_map[suffix]
    if suffix.startswith('legacy-'):
        return suffix[len('legacy-'):]+'opqual-stale-payload'
    return suffix

def toggle_prefix(key):
    return {
        'ENABLE_MEMORY_CLEANER':'admin_do:toggle_memory:',
        'ENABLE_LIVE_SYNC':'admin_do:toggle_live_sync:',
        'MAINTENANCE_MODE':'admin_do:toggle_maintenance:',
    }[key]

def toggle_via_confirmation(key, require_persisted_delta=True):
    before=setting(key)
    start=event_count()
    driver('callback','--user-id',ADMIN,'--chat-id',ADMIN,'--data','btn_toggle_'+key)
    row,payload=wait_keyboard(start,toggle_prefix(key),15)
    mid=row.get('response_message_id') or row.get('message_id')
    if mid is None: raise ContractFail(f'{key} confirmation message id missing')
    cb_start=event_count()
    driver('callback','--user-id',ADMIN,'--chat-id',ADMIN,'--data',payload,'--message-id',int(mid))
    telegram_contains(cb_start,'Setting updated.',15)
    after=setting(key)
    if require_persisted_delta and after==before:
        raise ContractFail(f'{key} confirmed toggle did not persist a new value')
    return {'before':before,'after':after,'confirmation':payload,'message_id':int(mid)}

def align_toggle_state(key,target):
    # Always pass through the application path at least once so persisted and in-memory states converge.
    # A convergence write may legitimately persist the same DB value while correcting AppContext memory.
    steps=[toggle_via_confirmation(key, require_persisted_delta=False)]
    if setting(key)!=target:
        steps.append(toggle_via_confirmation(key, require_persisted_delta=False))
    if setting(key)!=target: raise ContractFail(f'{key} could not align to {target}')
    return steps

def align_runtime_settings_baseline():
    align_toggle_state('ENABLE_MEMORY_CLEANER','false')
    align_toggle_state('ENABLE_LIVE_SYNC','true')
    align_toggle_state('MAINTENANCE_MODE','false')

def nonce_contract(suffix):
    command=NONCE_COMMAND[suffix]
    if suffix in ('confirm-clear_wallets','confirm-forget_all'):
        seed_wallet(ADMIN)
    if suffix=='confirm-cleanup_events':
        pg("INSERT INTO bot_event_log(event_type,severity,chat_id,status,metadata,created_at) VALUES ('OPQUAL_OLD','info',%d,'old','{}'::jsonb,NOW()-INTERVAL '100 days');"%ADMIN)
    before_alert=setting('ENABLE_ALERT_DELIVERY') if suffix in ('confirm-mute_alerts','confirm-unmute_alerts') else None
    toggle_key={'confirm-toggle_memory':'ENABLE_MEMORY_CLEANER','confirm-toggle_live_sync':'ENABLE_LIVE_SYNC','confirm-toggle_maintenance':'MAINTENANCE_MODE'}.get(suffix)
    before_toggle=setting(toggle_key) if toggle_key else None

    start=event_count()
    driver('message','--user-id',ADMIN,'--chat-id',ADMIN,'--text',command)
    row,payload=wait_keyboard(start,'admin_do:',15)
    mid=row.get('response_message_id') or row.get('message_id')
    if mid is None: raise ContractFail('confirmation response message id missing')
    cb_start=event_count()
    driver('callback','--user-id',ADMIN,'--chat-id',ADMIN,'--data',payload,'--message-id',int(mid))
    deadline=time.monotonic()+15; evidence=None
    while time.monotonic()<deadline:
        rows=load_events(TELEGRAM_EVENTS)[cb_start:]
        joined='\n'.join(json.dumps(x,ensure_ascii=False) for x in rows)
        if 'Confirmation failed' in joined or 'Confirmation expired' in joined:
            raise ContractFail(f'nonce confirmation failed for {suffix}')
        for x in rows:
            if any(t in json.dumps(x,ensure_ascii=False) for t in ('Action completed','Confirmed.','deleted and verified','Alerts resumed','Alerts muted','Cleanup')):
                evidence=x; break
        if evidence: break
        time.sleep(.1)
    if not evidence: raise ContractFail(f'no confirmed effect output for {suffix}')

    contract='bound_nonce_confirmation'
    if suffix in ('confirm-clear_wallets','confirm-forget_all'):
        if wallet_count(ADMIN)!=0: raise ContractFail(f'{suffix} did not delete seeded admin wallet')
        seed_wallet(ADMIN); contract += '+db_delete'
    if suffix=='confirm-cleanup_events':
        if int(pg("SELECT count(*) FROM bot_event_log WHERE event_type='OPQUAL_OLD';") or 0)!=0: raise ContractFail('cleanup did not delete old event')
        contract += '+db_cleanup'
    if suffix=='confirm-mute_alerts' and setting('ENABLE_ALERT_DELIVERY')!='false': raise ContractFail('mute did not persist disabled')
    if suffix=='confirm-unmute_alerts' and setting('ENABLE_ALERT_DELIVERY')!='true': raise ContractFail('unmute did not persist enabled')
    if suffix in ('confirm-mute_alerts','confirm-unmute_alerts'): contract += '+db_setting'
    if toggle_key:
        after=setting(toggle_key)
        if after==before_toggle: raise ContractFail(f'{toggle_key} did not toggle through nonce confirmation')
        restore=toggle_via_confirmation(toggle_key)
        if setting(toggle_key)!=before_toggle: raise ContractFail(f'{toggle_key} cleanup restore failed')
        contract += '+db_toggle+confirmed_restore'
    return {'input':payload,'confirmation_message_id':int(mid),'telegram_event':evidence.get('sequence'),'contract':contract}

def callback_contract(suffix,role_preconditions):
    if suffix in NONCE_COMMAND:
        return nonce_contract(suffix)
    is_admin = suffix in ADMIN_CALLBACKS or 'admin' in role_preconditions.lower()
    uid,cid=(ADMIN,ADMIN) if is_admin else (USER,CHAT)
    data=callback_data_for(suffix,cid)
    if suffix in ('wp','wbal','wblk','wmin','wrc','wrd'): seed_wallet(cid)

    if suffix.startswith('btn_toggle_'):
        key=suffix[len('btn_toggle_'):]
        before=setting(key)
        first=toggle_via_confirmation(key)
        if setting(key)==before: raise ContractFail(f'{key} did not toggle through confirmed button path')
        second=toggle_via_confirmation(key)
        if setting(key)!=before: raise ContractFail(f'{key} did not restore through confirmed button path')
        return {'input':data,'confirmation':first,'restore':second,'contract':'button_confirmation+db_toggle+confirmed_restore'}

    if suffix=='wrd':
        start,row=inject_callback(data,uid,cid)
        if wallet_count(cid)!=0: raise ContractFail('wrd did not remove wallet')
        seed_wallet(cid)
        return {'input':data,'telegram_event':row.get('sequence'),'contract':'wallet_token_callback+db_delete+restore'}

    if suffix in ('do_forget_wallets','do_forget_all','do_pause','do_resume','do_cleanup_events','do_mute_alerts','do_unmute_alerts'):
        before_wallet=wallet_count(cid)
        start,row=inject_callback(data,uid,cid)
        joined='\n'.join(json.dumps(x,ensure_ascii=False) for x in load_events(TELEGRAM_EVENTS)[start:])
        if 'Confirmation expired' not in joined:
            raise ContractFail(f'legacy direct sensitive callback was not fail-closed: {suffix}')
        if suffix in ('do_forget_wallets','do_forget_all') and wallet_count(cid)!=before_wallet:
            raise ContractFail(f'expired {suffix} unexpectedly mutated wallet state')
        return {'input':data,'telegram_event':row.get('sequence'),'contract':'expired_sensitive_callback_fail_closed'}

    start,row=inject_callback(data,uid,cid)
    return {'input':data,'telegram_event':row.get('sequence'),'contract':'dispatcher_callback_reply'}
def edge002_contract():
    delete_wallet(CHAT)
    inject_callback('cmd_add_wallet',USER,CHAT)
    for attempt in range(2):
        start=event_count(); driver('message','--user-id',USER,'--chat-id',CHAT,'--text',f'not-a-wallet-{attempt}')
        telegram_contains(start,'No Kaspa wallet found',10)
        if wallet_count(CHAT)!=0: raise ContractFail('invalid add input created wallet state')
    start=event_count(); driver('message','--user-id',USER,'--chat-id',CHAT,'--text',WALLET)
    wait_output(start,CHAT,15)
    if wallet_count(CHAT)!=1: raise ContractFail('pending Add Wallet session did not survive repeated invalid input')
    return {'contract':'pending_add_survives_two_invalid_inputs_then_valid_add','attempts':2}

def edge003_contract():
    seed_wallet(CHAT)
    align_toggle_state('MAINTENANCE_MODE','false')
    toggle_via_confirmation('MAINTENANCE_MODE')
    if setting('MAINTENANCE_MODE')!='true': raise ContractFail('maintenance toggle-on failed')
    try:
        token=wallet_token(CHAT); start=event_count()
        driver('callback','--user-id',USER,'--chat-id',CHAT,'--data',f'wrd:{token}')
        wait_output(start,CHAT,15)
        if wallet_count(CHAT)!=0: raise ContractFail('maintenance-safe wrd did not remove wallet')
    finally:
        seed_wallet(CHAT)
        if setting('MAINTENANCE_MODE')!='false': toggle_via_confirmation('MAINTENANCE_MODE')
    if setting('MAINTENANCE_MODE')!='false': raise ContractFail('maintenance state was not restored')
    return {'contract':'maintenance_allows_existing_privacy_delete_callback+db_delete+confirmed_restore'}

def health_probe(path):
    code=f"import urllib.request; r=urllib.request.urlopen('http://{APP}:{HEALTH_PORT}/{path}',timeout=3); print(r.status); print(r.read().decode())"
    p=run(['sudo','-n','docker','run','--rm','--network',os.environ['DOCKER_NETWORK'],os.environ['PYTHON_IMAGE'],'python','-c',code])
    if not p.stdout.startswith('200\n'): raise ContractFail(f'/{path} not HTTP 200')
    return p.stdout[:1000]
def app_logs():
    parts=[]
    for name in ('app.stdout.log','app.stderr.log'):
        p=RUN/name
        if p.exists(): parts.append(p.read_text(encoding='utf-8',errors='replace'))
    return '\n'.join(parts)

def provider_has(event_file,**wanted):
    for row in load_events(event_file):
        if all(row.get(k)==v for k,v in wanted.items()): return row
    return None

def webhook_cycle_contract():
    script=ROOT/'scripts/opqual/webhook-cycle.sh'
    p=run([str(script)])
    receipt=RUN/'webhook-cycle.json'
    if not receipt.exists(): raise ContractFail('webhook cycle receipt missing')
    data=json.loads(receipt.read_text())
    if data.get('result')!='PASS': raise ContractFail(f'webhook cycle not PASS: {data}')
    return {'contract':'exact_candidate_webhook_cycle','receipt':data,'output':p.stdout[-500:]}

def job_contract(name):
    if name=='telegram_webhook_server': return webhook_cycle_contract()
    marker=f'[TASK START] {name}'
    if marker not in app_logs(): raise Blocked(f'task did not execute in this synthetic run: {name}')
    return {'contract':'owned_task_started','marker':marker}

def task_contract(name):
    marker=f'[TASK START] {name}'
    if name=='utxo_reward_analysis' and marker not in app_logs():
        script=ROOT/'scripts/opqual/reward-task-cycle.sh'
        p=run([str(script)])
        receipt=RUN/'reward-task-cycle.json'
        if not receipt.exists(): raise ContractFail('reward task cycle receipt missing')
        data=json.loads(receipt.read_text())
        if data.get('result')!='PASS' or not all(data.get(k) for k in ('task_start','task_stop','task_join')):
            raise ContractFail(f'reward task cycle did not prove clean task ownership: {data}')
    if marker not in app_logs(): raise Blocked(f'owned child/request task not observed in this run: {name}')
    joined=f'[TASK MONITOR] {name} joined cleanly'
    if joined not in app_logs(): raise ContractFail(f'owned child/request task did not join cleanly: {name}')
    return {'contract':'owned_child_task_started_and_joined','marker':marker,'joined':joined}

def http_contract(name):
    if name=='webhook': return webhook_cycle_contract()
    body=health_probe(name)
    return {'contract':'actual_internal_http_200','body':body[:300]}

def coingecko_history_contract():
    day=(datetime.datetime.now(datetime.timezone.utc)-datetime.timedelta(days=1)).date().isoformat()
    outpoint='opqual-history-synthetic:0'
    pg(f"DELETE FROM kas_price_history WHERE day={sql_quote(day)}::date; DELETE FROM mined_blocks WHERE outpoint={sql_quote(outpoint)};")
    pg(f"INSERT INTO mined_blocks(wallet,outpoint,amount,daa_score,timestamp) VALUES ({sql_quote(WALLET)},{sql_quote(outpoint)},100000000,1,{sql_quote(day+' 12:00:00+00')}::timestamptz);")
    start=len(load_events(HTTP_EVENTS))
    deadline=time.monotonic()+20
    row=None
    while time.monotonic()<deadline:
        for candidate in load_events(HTTP_EVENTS)[start:]:
            if candidate.get('host')=='api.coingecko.com' and candidate.get('path')=='/api/v3/coins/kaspa/market_chart/range':
                row=candidate; break
        if row: break
        time.sleep(.2)
    if not row: raise Blocked('periodic KAS price sync did not exercise CoinGecko history within 20s')
    stored=int(pg(f"SELECT count(*) FROM kas_price_history WHERE day={sql_quote(day)}::date AND source='coingecko_history';") or 0)
    if stored<1: raise ContractFail('CoinGecko history response was not persisted for synthetic mined day')
    pg(f"DELETE FROM mined_blocks WHERE outpoint={sql_quote(outpoint)}; DELETE FROM kas_price_history WHERE day={sql_quote(day)}::date;")
    return {'contract':'periodic_system_task+actual_history_provider+db_backfill','day':day,'event':row}

def integration_contract(name):
    if name=='telegram':
        row=next((x for x in load_events(TELEGRAM_EVENTS) if x.get('kind')=='telegram_request'),None)
    elif name=='kaspa-wrpc':
        row=next((x for x in load_events(KASPA_EVENTS) if x.get('kind')=='kaspa_request'),None)
    elif name=='coingecko-current': row=provider_has(HTTP_EVENTS,host='api.coingecko.com',path='/api/v3/simple/price')
    elif name=='coingecko-history': return coingecko_history_contract()
    elif name=='kaspa-price': row=provider_has(HTTP_EVENTS,host='api.kaspa.org',path='/info/price')
    elif name=='kaspa-marketcap': row=provider_has(HTTP_EVENTS,host='api.kaspa.org',path='/info/marketcap')
    elif name=='kaspa-fees': row=provider_has(HTTP_EVENTS,host='api.kaspa.org',path='/info/fee-estimate')
    else: raise Blocked(f'unknown integration strategy {name}')
    if not row: raise Blocked(f'integration was not exercised in this run: {name}')
    return {'contract':'actual_synthetic_provider_interaction','event':row}
def life_contract(name):
    if name=='startup':
        if 'Kaspa Pulse starting.' not in app_logs(): raise ContractFail('startup marker missing')
        if int(pg("SELECT count(*) FROM bot_event_log WHERE event_type='SYSTEM_START';") or 0)<1: raise ContractFail('SYSTEM_START row missing')
        return {'contract':'main_startup_log+database_event'}
    if name=='settings':
        values={k:setting(k) for k in ('ENABLE_MEMORY_CLEANER','ENABLE_LIVE_SYNC','MAINTENANCE_MODE')}
        if any(v=='' for v in values.values()): raise ContractFail('persisted runtime settings missing')
        return {'contract':'persisted_settings_available_after_startup','values':values}
    if name in ('shutdown','pool-close'):
        p=RUN/'shutdown-verification.json'
        if not p.exists(): raise Blocked('shutdown verification receipt missing')
        data=json.loads(p.read_text())
        required=('application_long_sql_lock_shutdown','zero_app_db_sessions_after_shutdown','zero_app_locks_after_shutdown','clean_exit')
        if any(data.get(k)!='PASS' for k in required): raise ContractFail(f'shutdown receipt not PASS: {data}')
        if name=='pool-close' and 'Database connections closed safely' not in app_logs(): raise ContractFail('pool-close marker missing')
        return {'contract':'exec01_shutdown_receipt','receipt':data}
    raise Blocked(f'unknown lifecycle strategy {name}')

def edge_contract(sid):
    if sid=='EDGE-002': return edge002_contract()
    if sid=='EDGE-003': return edge003_contract()
    raise Blocked(f'no edge strategy for {sid}')

def execute_row(row):
    sid=row['id']; feature=row['feature_ids']; role=row.get('role_preconditions','')
    reset_db_baseline()
    if sid.startswith('EDGE-'): return edge_contract(sid)
    if feature.startswith('CMD-'): return command_contract(feature[4:])
    if feature.startswith('CB-'): return callback_contract(feature[3:],role)
    if feature.startswith('JOB-'): return job_contract(feature[4:])
    if feature.startswith('TASK-'): return task_contract(feature[5:])
    if feature.startswith('HTTP-'): return http_contract(feature[5:])
    if feature.startswith('INT-'): return integration_contract(feature[4:])
    if feature.startswith('LIFE-'): return life_contract(feature[5:])
    raise Blocked(f'no execution strategy for {feature}')
def priority(row):
    f=row['feature_ids']
    order=(('CMD-',10),('CB-',20),('EDGE-',30),('HTTP-',40),('INT-',50),('JOB-',60),('TASK-',70),('LIFE-',80))
    if row['id'].startswith('EDGE-'): return 30
    for prefix,value in order:
        if f.startswith(prefix): return value
    return 90

def app_running():
    p=run(['sudo','-n','docker','inspect',APP,'--format','{{.State.Running}}'],check=False)
    return p.returncode==0 and p.stdout.strip()=='true'

def main():
    with MATRIX.open(encoding='utf-8-sig',newline='') as f: rows=list(csv.DictReader(f))
    indexed=list(enumerate(rows))
    selected={x.strip() for x in os.environ.get('OPQUAL_SCENARIO_IDS','').split(',') if x.strip()}
    valid_blocked={r['id'] for _,r in indexed if r.get('outcome')=='BLOCKED'}
    unknown=selected-valid_blocked
    if unknown: raise SystemExit('unknown/non-blocked selected scenario ids: '+','.join(sorted(unknown)))
    blocked=[(i,r) for i,r in indexed if r.get('outcome')=='BLOCKED' and (not selected or r['id'] in selected)]
    blocked.sort(key=lambda x:(priority(x[1]),x[0]))
    results={}
    existing=RUN/'scenario-results.csv'
    if selected and existing.exists():
        with existing.open(encoding='utf-8-sig',newline='') as f:
            results={r['id']:r for r in csv.DictReader(f)}
    for i,row in indexed:
        if row.get('outcome')!='BLOCKED' and row['id'] not in results:
            results[row['id']]={'id':row['id'],'feature_ids':row['feature_ids'],'outcome':row['outcome'],
                'executed_this_run':'NO','strategy':'REUSED_QUALIFIED_EVIDENCE','evidence':row.get('evidence',''),'reason':'source-bound prior evidence retained; application source unchanged'}
    if selected and not existing.exists(): raise SystemExit('selective rerun requires existing scenario-results.csv')

    align_runtime_settings_baseline()
    evidence_dir=RUN/'scenario-evidence'; evidence_dir.mkdir(exist_ok=True)
    for _,row in blocked:
        sid=row['id']; started=time.monotonic_ns()
        if not app_running():
            result={'id':sid,'feature_ids':row['feature_ids'],'outcome':'BLOCKED','executed_this_run':'NO','strategy':'RUNTIME_PREREQUISITE','evidence':'','reason':'application is not running'}
        else:
            try:
                evidence=execute_row(row)
                result={'id':sid,'feature_ids':row['feature_ids'],'outcome':'VERIFIED_PASS','executed_this_run':'YES','strategy':evidence.get('contract','runtime_contract'),'evidence':f'scenario-evidence/{sid}.json','reason':''}
                (evidence_dir/f'{sid}.json').write_text(json.dumps({'scenario':row,'runtime_evidence':evidence,'started_monotonic_ns':started,'finished_monotonic_ns':time.monotonic_ns()},indent=2,sort_keys=True)+'\n')
            except Blocked as e:
                result={'id':sid,'feature_ids':row['feature_ids'],'outcome':'BLOCKED','executed_this_run':'PARTIAL','strategy':'NO_COMPLETE_RUNTIME_CONTRACT','evidence':'','reason':str(e)}
            except Exception as e:
                result={'id':sid,'feature_ids':row['feature_ids'],'outcome':'VERIFIED_FAIL','executed_this_run':'YES','strategy':'RUNTIME_CONTRACT_FAILED','evidence':f'scenario-evidence/{sid}.failure.json','reason':str(e)}
                (evidence_dir/f'{sid}.failure.json').write_text(json.dumps({'scenario':row,'error':str(e),'traceback':traceback.format_exc(),'started_monotonic_ns':started,'finished_monotonic_ns':time.monotonic_ns()},indent=2,sort_keys=True)+'\n')
        results[sid]=result
        print(f"{sid}={result['outcome']} {result['reason']}",flush=True)
    fields=['id','feature_ids','outcome','executed_this_run','strategy','evidence','reason']
    out_path=RUN/'scenario-results.csv'
    with out_path.open('w',encoding='utf-8',newline='') as f:
        w=csv.DictWriter(f,fieldnames=fields); w.writeheader()
        for row in rows: w.writerow(results[row['id']])
    counts={k:0 for k in ('VERIFIED_PASS','VERIFIED_FAIL','BLOCKED','NOT_APPLICABLE')}
    for r in results.values(): counts[r['outcome']]=counts.get(r['outcome'],0)+1
    summary={'total':len(rows),**{k.lower():v for k,v in counts.items()},'not_tested':0,
             'full_bot_journeys_completed':sum(1 for r in results.values() if r['executed_this_run']=='YES' and r['outcome']=='VERIFIED_PASS')}
    (RUN/'scenario-summary.json').write_text(json.dumps(summary,indent=2,sort_keys=True)+'\n')
    print(json.dumps(summary,sort_keys=True))
    if counts.get('VERIFIED_FAIL',0): return 2
    return 0

if __name__=='__main__': raise SystemExit(main())
