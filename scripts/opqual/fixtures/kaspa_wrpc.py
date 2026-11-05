"""Small RFC6455 and pinned Kaspa wRPC JSON fixture, never connects outward."""
import base64
import hashlib
import json
import os
import signal
import struct
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

LOCK = threading.Lock()
EVENT_FILE = Path(os.environ.get("OPQUAL_EVENT_DIR", "/evidence")) / "kaspa-events.jsonl"
CONTROL_FILE = Path(os.environ.get("OPQUAL_CONTROL_DIR", "/control")) / "kaspa.json"
ZERO_HASH = "00" * 32
BASE_SCORE = 1000000

def mode_for(method):
    try:
        root = json.loads(CONTROL_FILE.read_text())
    except Exception:
        root = {}
    cfg = root.get(method, root.get("default", {})) if isinstance(root, dict) else {}
    return cfg if isinstance(cfg, dict) else {}

def event(value):
    with LOCK:
        value["timestamp"] = time.time()
        encoded = json.dumps(value, ensure_ascii=False)
        with EVENT_FILE.open("a", encoding="utf-8") as out:
            out.write(encoded + "\n")
            out.flush()
        print(encoded, flush=True)

def control():
    try:
        value = json.loads(CONTROL_FILE.read_text(encoding="utf-8"))
        return value if isinstance(value, dict) else {}
    except (FileNotFoundError, ValueError, TypeError):
        return {}

def result_for(method):
    score = BASE_SCORE + int(time.monotonic())
    if method == "getServerInfo":
        return {"rpcApiVersion": 1, "rpcApiRevision": 0,
                "serverVersion": "local-fixture-cfafeb4", "networkId": "mainnet",
                "hasUtxoIndex": True, "isSynced": True, "virtualDaaScore": score}
    if method == "getSyncStatus":
        return {"isSynced": True}
    if method == "getBlockDagInfo":
        return {"network": "mainnet", "blockCount": 1, "headerCount": 1,
                "tipHashes": [ZERO_HASH], "difficulty": 1.0,
                "pastMedianTime": int(time.time() * 1000),
                "virtualParentHashes": [ZERO_HASH],
                "pruningPointHash": ZERO_HASH, "virtualDaaScore": score,
                "sink": ZERO_HASH}
    if method == "getCoinSupply":
        return {"maxSompi": 2870400000000000000, "circulatingSompi": 2000000000000000000}
    if method == "getUtxosByAddresses":
        entries = control().get("utxos", [])
        return {"entries": entries if isinstance(entries, list) else []}
    if method == "getConnectedPeerInfo":
        return {"peerInfo": []}
    if method == "getBlockCount":
        return {"blockCount": 1, "headerCount": 1}
    if method == "getSink":
        return {"sink": ZERO_HASH}
    if method == "getCurrentNetwork":
        return {"currentNetwork": "mainnet"}
    if method == "estimateNetworkHashesPerSecond":
        return {"networkHashesPerSecond": 1000000}
    if method == "subscribe":
        return {"id": 1}
    if method in ("unsubscribe", "notifyVirtualDaaScoreChanged", "ping"):
        return {}
    raise KeyError(method)

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.headers.get("Upgrade", "").lower() != "websocket":
            self.send_response(200)
            payload = b"local kaspa fixture\n"
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        key = self.headers.get("Sec-WebSocket-Key", "")
        accept = base64.b64encode(hashlib.sha1(
            (key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        self.wfile.flush()
        self.send_lock = threading.Lock()
        self.alive = threading.Event()
        self.alive.set()
        self.subscribed = False
        event({"kind": "websocket_connected"})
        try:
            self.run_frames()
        except (EOFError, OSError, ValueError) as error:
            event({"kind": "websocket_closed", "reason": type(error).__name__})
        finally:
            self.alive.clear()
            self.close_connection = True

    def exact(self, size):
        value = self.rfile.read(size)
        if len(value) != size:
            raise EOFError()
        return value

    def send_frame(self, payload, opcode=1):
        if isinstance(payload, dict):
            payload = json.dumps(payload).encode()
        size = len(payload)
        prefix = bytes([0x80 | opcode])
        if size < 126:
            prefix += bytes([size])
        elif size <= 65535:
            prefix += bytes([126]) + struct.pack("!H", size)
        else:
            prefix += bytes([127]) + struct.pack("!Q", size)
        with self.send_lock:
            self.wfile.write(prefix + payload)
            self.wfile.flush()

    def notifications(self):
        while self.alive.is_set() and self.subscribed:
            try:
                self.send_frame({"method": "virtualDaaScoreChangedNotification",
                                 "params": {"virtualDaaScore": BASE_SCORE + int(time.monotonic())}})
            except OSError:
                return
            time.sleep(5)

    def run_frames(self):
        fragments = bytearray()
        while self.alive.is_set():
            first, second = self.exact(2)
            opcode = first & 15
            final = bool(first & 128)
            size = second & 127
            if size == 126:
                size = struct.unpack("!H", self.exact(2))[0]
            elif size == 127:
                size = struct.unpack("!Q", self.exact(8))[0]
            if size > 1024 * 1024:
                raise ValueError("oversized frame")
            mask = self.exact(4) if second & 128 else None
            payload = self.exact(size)
            if mask:
                payload = bytes(val ^ mask[index % 4] for index, val in enumerate(payload))
            if opcode == 8:
                self.send_frame(payload, opcode=8)
                return
            if opcode == 9:
                self.send_frame(payload, opcode=10)
                continue
            if opcode == 10:
                continue
            if opcode not in (0, 1):
                raise ValueError("unsupported websocket opcode")
            fragments.extend(payload)
            if not final:
                continue
            request = json.loads(fragments)
            fragments.clear()
            method = request.get("method")
            cfg = mode_for(method)
            mode = str(cfg.get("mode", "success"))
            summary = {"kind": "kaspa_request", "method": method, "mode": mode,
                       "params": request.get("params"), "supported": True}
            if mode == "timeout":
                time.sleep(max(float(cfg.get("delay_ms", 0)) / 1000.0, 30.0))
            if mode == "disconnect":
                event(summary | {"disconnect": True})
                return
            if mode == "malformed":
                event(summary | {"malformed": True})
                if request.get("id") is not None:
                    self.send_frame(b"{not-json")
                continue
            try:
                params = result_for(method)
                if mode == "invalid":
                    if method == "getSyncStatus": params = {"isSynced": "invalid"}
                    elif method == "getServerInfo": params = {"networkId": "invalid", "isSynced": False}
                    else: params = {"syntheticInvalid": True}
                response = {"id": request.get("id"), "params": params}
                if mode in ("error", "5xx", "4xx", "rate_limit"):
                    response = {"id": request.get("id"), "error": {"code": 429 if mode == "rate_limit" else 1, "message": f"synthetic {mode}"}}
            except KeyError:
                summary["supported"] = False
                response = {"id": request.get("id"),
                            "error": {"code": 1, "message": "Unsupported local fixture method",
                                      "data": {"method": method}}}
            event(summary)
            if request.get("id") is not None:
                self.send_frame(response)
            if method in ("subscribe", "unsubscribe", "notifyVirtualDaaScoreChanged"):
                enabled = (method == "subscribe" or (method == "notifyVirtualDaaScoreChanged" and
                           request.get("params", {}).get("command", "start") in ("Start", "start", 0)))
                was_subscribed = self.subscribed
                self.subscribed = enabled
                if enabled and not was_subscribed:
                    threading.Thread(target=self.notifications, daemon=True).start()

    def log_message(self, *_):
        pass

if __name__ == "__main__":
    event({"kind": "fixture_started", "protocol": "workflow-rpc-0.18.0/pinned-cfafeb4"})
    server = ThreadingHTTPServer(("0.0.0.0", 17110), Handler)
    def _stop(signum, _frame):
        event({"kind": "fixture_signal", "signal": signum})
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, _stop)
    signal.signal(signal.SIGINT, _stop)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        event({"kind": "fixture_stopped"})
