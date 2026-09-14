#!/usr/bin/env python3
"""Synthetic Telegram Bot API fixture for isolated operational qualification."""
import itertools, json, os, signal, ssl, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs

CONTROL = Path(os.environ.get("OPQUAL_CONTROL_DIR", "/control"))
EVENTS = Path(os.environ.get("OPQUAL_EVENT_DIR", "/evidence"))
EVENTS.mkdir(parents=True, exist_ok=True)
EVENT_FILE = EVENTS / "telegram-events.jsonl"
UPDATE_FILE = CONTROL / "telegram-updates.jsonl"
CONTROL_FILE = CONTROL / "telegram.json"
LOCK = threading.Lock()
SEQ = itertools.count(1)
MSG_IDS = itertools.count(10001)
BOT = {"id":1234567890,"is_bot":True,"first_name":"OpQualBot","username":"opqual_bot",
       "can_join_groups":True,"can_read_all_group_messages":False,"supports_inline_queries":False,
       "can_connect_to_business":False,"has_main_web_app":False}
WEBHOOK_URL = ""


def emit(obj):
    with LOCK:
        obj = dict(obj, sequence=next(SEQ), timestamp=time.time())
        with EVENT_FILE.open("a", encoding="utf-8") as f:
            f.write(json.dumps(obj, ensure_ascii=False) + "\n"); f.flush()
        print(json.dumps(obj, ensure_ascii=False), flush=True)


def control():
    try:
        data=json.loads(CONTROL_FILE.read_text())
        return data if isinstance(data,dict) else {}
    except Exception:
        return {}


def updates(offset):
    rows=[]
    if UPDATE_FILE.exists():
        for line in UPDATE_FILE.read_text().splitlines():
            if not line.strip(): continue
            try: item=json.loads(line)
            except json.JSONDecodeError: continue
            if int(item.get("update_id",0)) >= offset: rows.append(item)
    return rows
class Handler(BaseHTTPRequestHandler):
    def read_data(self):
        size=int(self.headers.get("Content-Length","0") or 0)
        raw=self.rfile.read(size) if size else b""
        ctype=self.headers.get("Content-Type","")
        if "application/json" in ctype:
            try: value=json.loads(raw or b"{}")
            except Exception: value={}
        else:
            try: value={k:v[-1] for k,v in parse_qs(raw.decode()).items()}
            except Exception: value={}
        return value if isinstance(value,dict) else {}

    def send_json(self, status, body):
        payload=json.dumps(body,ensure_ascii=False).encode()
        self.send_response(status); self.send_header("Content-Type","application/json")
        self.send_header("Content-Length",str(len(payload))); self.end_headers(); self.wfile.write(payload)

    def handle_request(self):
        global WEBHOOK_URL
        data=self.read_data()
        method=self.path.split("?",1)[0].rsplit("/",1)[-1].lower()
        cfg=control().get(method,{})
        mode=str(cfg.get("mode","success"))
        delay=float(cfg.get("delay_ms",0))/1000.0
        if mode == "timeout": delay=max(delay,30.0)
        if delay: time.sleep(delay)
        summary={"kind":"telegram_request","method":method,"mode":mode}
        for key in ("chat_id","message_id","text","callback_query_id","scope","commands","reply_markup","parse_mode","offset"):
            if key in data: summary[key]=data[key]

        if mode == "malformed":
            emit(dict(summary,status_code=200,malformed=True))
            raw=b"{not-json"; self.send_response(200); self.send_header("Content-Length",str(len(raw))); self.end_headers(); self.wfile.write(raw); return
        if mode in ("4xx","5xx","rate_limit"):
            status=429 if mode=="rate_limit" else (400 if mode=="4xx" else 503)
            body={"ok":False,"error_code":status,"description":f"synthetic {mode}"}
            if mode=="rate_limit": body["parameters"]={"retry_after":int(cfg.get("retry_after",1))}
            emit(dict(summary,status_code=status)); self.send_json(status,body); return

        status=200
        if method == "getme": result=BOT
        elif method == "getupdates":
            try: offset=int(data.get("offset",0) or 0)
            except Exception: offset=0
            result=updates(offset); summary["returned_updates"]=len(result)
        elif method == "getwebhookinfo": result={"url":WEBHOOK_URL,"has_custom_certificate":False,"pending_update_count":0}
        elif method == "setwebhook": WEBHOOK_URL=str(data.get("url","")); result=True
        elif method == "deletewebhook": WEBHOOK_URL=""; result=True
        elif method in ("sendmessage","editmessagetext","editmessagereplymarkup"):
            try: chat_id=int(data.get("chat_id",0)); mid=int(data.get("message_id",next(MSG_IDS)))
            except Exception: chat_id=0; mid=next(MSG_IDS)
            text=str(data.get("text",""))
            units=len(text.encode("utf-16-le"))//2
            summary["utf16_units"]=units
            if units>4096:
                status=400; result=None
                body={"ok":False,"error_code":400,"description":"Bad Request: message is too long"}
                emit(dict(summary,status_code=status,rejected=True)); self.send_json(status,body); return
            result={"message_id":mid,"date":int(time.time()),"chat":{"id":chat_id,"type":"private"},"from":BOT,"text":text}
            summary["response_message_id"]=mid
        elif method in ("setmycommands","deletemycommands","answercallbackquery","deletemessage","sendchataction"):
            result=True
        elif method == "getmycommands": result=[]
        else:
            status=400; result=None
            body={"ok":False,"error_code":400,"description":"unsupported synthetic Telegram method"}
            emit(dict(summary,status_code=status,rejected=True)); self.send_json(status,body); return

        emit(dict(summary,status_code=status,rejected=False))
        self.send_json(status,{"ok":True,"result":result})

    do_GET=handle_request
    do_POST=handle_request
    def log_message(self,*_): pass


if __name__ == "__main__":
    port=int(os.environ.get("OPQUAL_TELEGRAM_PORT","443"))
    server=ThreadingHTTPServer(("0.0.0.0",port),Handler)
    ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(os.environ.get("OPQUAL_TLS_CERT","/certs/server.crt"),os.environ.get("OPQUAL_TLS_KEY","/certs/server.key"))
    server.socket=ctx.wrap_socket(server.socket,server_side=True)
    emit({"kind":"fixture_started","fixture":"telegram","port":port})
    def _stop(signum, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, _stop)
    signal.signal(signal.SIGINT, _stop)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
