#!/usr/bin/env python3
"""Deterministic synthetic Telegram update injector/event observer for OPQUAL."""
import argparse,json,os,time
from pathlib import Path


def load_rows(path):
    if not path.exists(): return []
    rows=[]
    for line in path.read_text(encoding='utf-8').splitlines():
        if not line.strip(): continue
        try: rows.append(json.loads(line))
        except json.JSONDecodeError: continue
    return rows


def append_jsonl(path,obj):
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.open('a',encoding='utf-8') as f:
        f.write(json.dumps(obj,ensure_ascii=False,sort_keys=True)+'\n'); f.flush(); os.fsync(f.fileno())


def next_update_id(path):
    values=[int(x.get('update_id',0)) for x in load_rows(path)]
    return max(values+[10000])+1


def user(uid): return {'id':uid,'is_bot':False,'first_name':'OpQualUser','username':'opqual_user'}
def chat(cid): return {'id':cid,'type':'private','first_name':'OpQualUser'}


def make_message(path,uid,cid,text=None,nontext=False):
    update_id=next_update_id(path); mid=update_id
    msg={'message_id':mid,'date':int(time.time()),'chat':chat(cid),'from':user(uid)}
    if nontext:
        msg['photo']=[{'file_id':'opqual-file','file_unique_id':'opqual-unique','width':1,'height':1}]
    else: msg['text']=text or ''
    return {'update_id':update_id,'message':msg}


def make_callback(path,uid,cid,data,message_id=None):
    update_id=next_update_id(path); mid=message_id if message_id is not None else update_id
    msg={'message_id':mid,'date':int(time.time()),'chat':chat(cid),'from':{'id':1234567890,'is_bot':True,'first_name':'OpQualBot','username':'opqual_bot'},'text':'synthetic callback origin'}
    return {'update_id':update_id,'callback_query':{'id':f'opqual-cb-{update_id}','from':user(uid),'message':msg,'chat_instance':'opqual-chat-instance','data':data}}
def wait_event(event_file,start,contains,timeout,methods):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        rows=load_rows(event_file)[start:]
        for row in rows:
            if methods and str(row.get('method','')).lower() not in methods: continue
            hay=json.dumps(row,ensure_ascii=False,sort_keys=True)
            if not contains or contains in hay: return row
        time.sleep(0.1)
    raise SystemExit(f'timeout waiting for Telegram event containing {contains!r}')


def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--control-dir',required=True)
    ap.add_argument('--event-dir',required=True)
    sub=ap.add_subparsers(dest='cmd',required=True)
    for name in ('message','nontext','callback'):
        p=sub.add_parser(name); p.add_argument('--user-id',type=int,required=True); p.add_argument('--chat-id',type=int,required=True)
        if name=='message': p.add_argument('--text',required=True)
        if name=='callback':
            p.add_argument('--data',required=True); p.add_argument('--message-id',type=int)
    p=sub.add_parser('event-count')
    p=sub.add_parser('wait-text'); p.add_argument('--start',type=int,required=True); p.add_argument('--contains',default=''); p.add_argument('--timeout',type=float,default=10.0); p.add_argument('--methods',default='sendmessage,editmessagetext,editmessagereplymarkup,answercallbackquery')
    p=sub.add_parser('last-keyboard'); p.add_argument('--start',type=int,default=0)
    args=ap.parse_args()
    control=Path(args.control_dir); events=Path(args.event_dir)
    updates=control/'telegram-updates.jsonl'; event_file=events/'telegram-events.jsonl'
    if args.cmd=='event-count': print(len(load_rows(event_file))); return
    if args.cmd=='message': obj=make_message(updates,args.user_id,args.chat_id,args.text); append_jsonl(updates,obj); print(obj['update_id']); return
    if args.cmd=='nontext': obj=make_message(updates,args.user_id,args.chat_id,nontext=True); append_jsonl(updates,obj); print(obj['update_id']); return
    if args.cmd=='callback': obj=make_callback(updates,args.user_id,args.chat_id,args.data,args.message_id); append_jsonl(updates,obj); print(obj['update_id']); return
    if args.cmd=='wait-text':
        methods={x.strip().lower() for x in args.methods.split(',') if x.strip()}
        print(json.dumps(wait_event(event_file,args.start,args.contains,args.timeout,methods),ensure_ascii=False,sort_keys=True)); return
    if args.cmd=='last-keyboard':
        for row in reversed(load_rows(event_file)[args.start:]):
            if row.get('reply_markup') is not None:
                print(json.dumps(row['reply_markup'],ensure_ascii=False,sort_keys=True)); return
        raise SystemExit('no keyboard event found')

if __name__=='__main__': main()
