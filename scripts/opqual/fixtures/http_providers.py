#!/usr/bin/env python3
"""Synthetic HTTPS providers for Kaspa.org and CoinGecko; never connects outward."""
import json,signal,os,ssl,time
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path

CONTROL=Path(os.environ.get("OPQUAL_CONTROL_DIR","/control"))
EVENTS=Path(os.environ.get("OPQUAL_EVENT_DIR","/evidence")); EVENTS.mkdir(parents=True,exist_ok=True)
EVENT_FILE=EVENTS/"http-provider-events.jsonl"
CONTROL_FILE=CONTROL/"http.json"

def cfg_for(host,path):
    try: root=json.loads(CONTROL_FILE.read_text())
    except Exception: root={}
    if not isinstance(root,dict): root={}
    return root.get(f"{host}{path}",root.get(path,root.get("default",{}))) or {}

def emit(obj):
    obj=dict(obj,timestamp=time.time())
    with EVENT_FILE.open("a",encoding="utf-8") as f: f.write(json.dumps(obj,sort_keys=True)+"\n"); f.flush()
    print(json.dumps(obj,sort_keys=True),flush=True)

def success_body(host,path):
    now=int(time.time()*1000)
    if host=="api.kaspa.org" and path=="/info/price": return {"price":0.123456}
    if host=="api.kaspa.org" and path=="/info/marketcap": return {"marketcap":1234567890.0}
    if host=="api.kaspa.org" and path=="/info/fee-estimate":
        return {"priorityBucket":{"feerate":3.0},"normalBuckets":[{"feerate":2.0}],"lowBuckets":[{"feerate":1.0}]}
    if host=="api.coingecko.com" and path=="/api/v3/simple/price":
        return {"kaspa":{"usd":0.123456,"usd_market_cap":1234567890.0}}
    if host=="api.coingecko.com" and path=="/api/v3/coins/kaspa/market_chart/range":
        return {"prices":[[now-86400000,0.12],[now,0.123456]]}
    return None
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        host=self.headers.get("Host","").split(":",1)[0]
        path=self.path.split("?",1)[0]
        cfg=cfg_for(host,path); mode=str(cfg.get("mode","success"))
        delay=float(cfg.get("delay_ms",0))/1000.0
        if mode=="timeout": delay=max(delay,30.0)
        if delay: time.sleep(delay)
        status=200; body=success_body(host,path)
        if mode=="4xx": status=400; body={"error":"synthetic 4xx"}
        elif mode=="5xx": status=503; body={"error":"synthetic 5xx"}
        elif mode=="rate_limit": status=429; body={"error":"rate limited","retry_after":int(cfg.get("retry_after",1))}
        elif mode=="invalid":
            if path=="/info/fee-estimate": body={"priorityBucket":{"feerate":-3.0},"normalBuckets":[],"lowBuckets":[]}
            elif path=="/info/price": body={"price":-1}
            elif path=="/info/marketcap": body={"marketcap":0}
            else: body={"kaspa":{"usd":-1,"usd_market_cap":0}}
        elif mode=="missing": body={}
        if body is None: status=404; body={"error":"unsupported synthetic provider path"}
        emit({"kind":"http_provider_request","host":host,"path":path,"mode":mode,"status":status})
        if mode=="malformed":
            payload=b"{not-json"
        else:
            payload=json.dumps(body).encode()
        self.send_response(status); self.send_header("Content-Type","application/json")
        self.send_header("Content-Length",str(len(payload))); self.end_headers(); self.wfile.write(payload)
    def log_message(self,*_): pass

if __name__=="__main__":
    port=int(os.environ.get("OPQUAL_HTTP_PORT","443"))
    server=ThreadingHTTPServer(("0.0.0.0",port),Handler)
    ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(os.environ.get("OPQUAL_TLS_CERT","/certs/server.crt"),os.environ.get("OPQUAL_TLS_KEY","/certs/server.key"))
    server.socket=ctx.wrap_socket(server.socket,server_side=True)
    emit({"kind":"fixture_started","fixture":"http-providers","port":port})
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
