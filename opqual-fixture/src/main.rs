use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use clap::{Parser, ValueEnum};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use serde_json::{Map, Value, json};
use sha1::{Digest, Sha1};
use std::{
    fs::{self, OpenOptions},
    io::{BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    Telegram,
    Http,
    Kaspa,
    Probe,
}

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, default_value = "/control")]
    control_dir: PathBuf,
    #[arg(long, default_value = "/evidence")]
    event_dir: PathBuf,
    #[arg(long, default_value = "/certs/server.crt")]
    tls_cert: PathBuf,
    #[arg(long, default_value = "/certs/server.key")]
    tls_key: PathBuf,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    ca: Option<PathBuf>,
}

fn unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn append_jsonl(path: &Path, mut value: Value) -> Result<()> {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("timestamp".into(), json!(unix_seconds()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(&value)?)?;
    f.flush()?;
    Ok(())
}
fn read_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|x| serde_json::from_str(&x).ok())
        .unwrap_or_else(|| json!({}))
}
fn sleep_mode(cfg: &Value) {
    let mode = cfg.get("mode").and_then(Value::as_str).unwrap_or("success");
    let mut ms = cfg.get("delay_ms").and_then(Value::as_u64).unwrap_or(0);
    if mode == "timeout" {
        ms = ms.max(30_000);
    }
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms));
    }
}
fn tls_config(cert: &Path, key: &Path) -> Result<Arc<ServerConfig>> {
    let mut c =
        BufReader::new(fs::File::open(cert).with_context(|| format!("open {}", cert.display()))?);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut c).collect::<std::result::Result<_, _>>()?;
    let mut k =
        BufReader::new(fs::File::open(key).with_context(|| format!("open {}", key.display()))?);
    let key: PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut k)?.context("private key missing")?;
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(Arc::new(cfg))
}
#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    headers: Map<String, Value>,
    body: Vec<u8>,
}
fn read_http<R: Read>(stream: &mut R) -> Result<Request> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp)?;
        ensure!(n > 0, "unexpected EOF");
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        ensure!(buf.len() <= 1024 * 1024, "header too large");
    }
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let head = String::from_utf8(buf[..split].to_vec())?;
    let mut lines = head.split("\r\n");
    let first = lines.next().context("request line missing")?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    let mut headers = Map::new();
    let mut content_len = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_owned();
            if key == "content-length" {
                content_len = val.parse().unwrap_or(0);
            }
            headers.insert(key, json!(val));
        }
    }
    let mut body = buf[split..].to_vec();
    while body.len() < content_len {
        let n = stream.read(&mut tmp)?;
        ensure!(n > 0, "body EOF");
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_len);
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}
fn write_http<W: Write>(w: &mut W, status: u16, ctype: &str, body: &[u8]) -> Result<()> {
    let reason = match status {
        101 => "Switching Protocols",
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Status",
    };
    write!(
        w,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    w.write_all(body)?;
    w.flush()?;
    Ok(())
}
fn request_data(req: &Request) -> Map<String, Value> {
    let ctype = req
        .headers
        .get("content-type")
        .and_then(Value::as_str)
        .unwrap_or("");
    if ctype.contains("application/json") {
        serde_json::from_slice::<Value>(&req.body)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    } else {
        let text = String::from_utf8_lossy(&req.body);
        let mut m = Map::new();
        for pair in text.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                m.insert(k.to_owned(), json!(url_decode(v)));
            }
        }
        m
    }
}
fn url_decode(v: &str) -> String {
    let mut out = String::new();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(' ');
                i += 1
            }
            b'%' if i + 2 < b.len() => {
                if let Ok(x) = u8::from_str_radix(&v[i + 1..i + 3], 16) {
                    out.push(x as char);
                    i += 3
                } else {
                    out.push('%');
                    i += 1
                }
            }
            x => {
                out.push(x as char);
                i += 1
            }
        }
    }
    out
}

#[derive(Clone)]
struct TelegramState {
    seq: Arc<AtomicU64>,
    msg: Arc<AtomicU64>,
    webhook: Arc<Mutex<String>>,
    control: PathBuf,
    events: PathBuf,
}
fn telegram(cli: &Cli) -> Result<()> {
    let port = cli.port.unwrap_or(443);
    let cfg = tls_config(&cli.tls_cert, &cli.tls_key)?;
    let state = TelegramState {
        seq: Arc::new(AtomicU64::new(1)),
        msg: Arc::new(AtomicU64::new(10001)),
        webhook: Arc::new(Mutex::new(String::new())),
        control: cli.control_dir.clone(),
        events: cli.event_dir.clone(),
    };
    fs::create_dir_all(&state.events)?;
    append_jsonl(
        &state.events.join("telegram-events.jsonl"),
        json!({"kind":"fixture_started","fixture":"telegram","port":port}),
    )?;
    for incoming in TcpListener::bind(("0.0.0.0", port))?.incoming() {
        let tcp = incoming?;
        let c = Arc::clone(&cfg);
        let s = state.clone();
        thread::spawn(move || {
            if let Err(e) = telegram_conn(tcp, c, s) {
                eprintln!("telegram fixture: {e:#}")
            }
        });
    }
    Ok(())
}
fn telegram_conn(tcp: TcpStream, cfg: Arc<ServerConfig>, s: TelegramState) -> Result<()> {
    let conn = ServerConnection::new(cfg)?;
    let mut io = StreamOwned::new(conn, tcp);
    let req = read_http(&mut io)?;
    let data = request_data(&req);
    let method = req
        .path
        .split('?')
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let root = read_json(&s.control.join("telegram.json"));
    let cfg = root.get(&method).cloned().unwrap_or_else(|| json!({}));
    sleep_mode(&cfg);
    let mode = cfg.get("mode").and_then(Value::as_str).unwrap_or("success");
    let mut summary = json!({"kind":"telegram_request","method":method,"mode":mode,"sequence":s.seq.fetch_add(1,Ordering::Relaxed)});
    if let Some(o) = summary.as_object_mut() {
        for k in [
            "chat_id",
            "message_id",
            "text",
            "callback_query_id",
            "scope",
            "commands",
            "reply_markup",
            "parse_mode",
            "offset",
        ] {
            if let Some(v) = data.get(k) {
                o.insert(k.into(), v.clone());
            }
        }
    }
    if mode == "malformed" {
        summary["status_code"] = json!(200);
        summary["malformed"] = json!(true);
        append_jsonl(&s.events.join("telegram-events.jsonl"), summary)?;
        return write_http(&mut io, 200, "application/json", b"{not-json");
    }
    if ["4xx", "5xx", "rate_limit"].contains(&mode) {
        let status = if mode == "rate_limit" {
            429
        } else if mode == "4xx" {
            400
        } else {
            503
        };
        summary["status_code"] = json!(status);
        append_jsonl(&s.events.join("telegram-events.jsonl"), summary)?;
        let mut body =
            json!({"ok":false,"error_code":status,"description":format!("synthetic {mode}")});
        if mode == "rate_limit" {
            body["parameters"] =
                json!({"retry_after":cfg.get("retry_after").and_then(Value::as_u64).unwrap_or(1)});
        }
        return write_http(
            &mut io,
            status,
            "application/json",
            serde_json::to_string(&body)?.as_bytes(),
        );
    }
    let bot = json!({"id":1234567890i64,"is_bot":true,"first_name":"OpQualBot","username":"opqual_bot","can_join_groups":true,"can_read_all_group_messages":false,"supports_inline_queries":false,"can_connect_to_business":false,"has_main_web_app":false});
    let mut status = 200u16;
    let result = match method.as_str() {
        "getme" => bot.clone(),
        "getupdates" => {
            let off = data
                .get("offset")
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<i64>().ok())
                        .or_else(|| v.as_i64())
                })
                .unwrap_or(0);
            let rows = read_jsonl(&s.control.join("telegram-updates.jsonl"))
                .into_iter()
                .filter(|v| v.get("update_id").and_then(Value::as_i64).unwrap_or(0) >= off)
                .collect::<Vec<_>>();
            summary["returned_updates"] = json!(rows.len());
            json!(rows)
        }
        "getwebhookinfo" => {
            json!({"url":s.webhook.lock().unwrap().clone(),"has_custom_certificate":false,"pending_update_count":0})
        }
        "setwebhook" => {
            *s.webhook.lock().unwrap() = data
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            json!(true)
        }
        "deletewebhook" => {
            s.webhook.lock().unwrap().clear();
            json!(true)
        }
        "sendmessage" | "editmessagetext" | "editmessagereplymarkup" => {
            let chat = data
                .get("chat_id")
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<i64>().ok())
                        .or_else(|| v.as_i64())
                })
                .unwrap_or(0);
            let mid = data
                .get("message_id")
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<i64>().ok())
                        .or_else(|| v.as_i64())
                })
                .unwrap_or_else(|| s.msg.fetch_add(1, Ordering::Relaxed) as i64);
            let text = data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let units = text.encode_utf16().count();
            summary["utf16_units"] = json!(units);
            if units > 4096 {
                status = 400;
                summary["status_code"] = json!(status);
                summary["rejected"] = json!(true);
                append_jsonl(&s.events.join("telegram-events.jsonl"), summary)?;
                return write_http(&mut io,status,"application/json",serde_json::to_string(&json!({"ok":false,"error_code":400,"description":"Bad Request: message is too long"}))?.as_bytes());
            }
            summary["response_message_id"] = json!(mid);
            json!({"message_id":mid,"date":unix_seconds() as i64,"chat":{"id":chat,"type":"private"},"from":bot,"text":text})
        }
        "setmycommands"
        | "deletemycommands"
        | "answercallbackquery"
        | "deletemessage"
        | "sendchataction" => json!(true),
        "getmycommands" => json!([]),
        _ => {
            status = 400;
            summary["status_code"] = json!(status);
            summary["rejected"] = json!(true);
            append_jsonl(&s.events.join("telegram-events.jsonl"), summary)?;
            return write_http(&mut io,status,"application/json",serde_json::to_string(&json!({"ok":false,"error_code":400,"description":"unsupported synthetic Telegram method"}))?.as_bytes());
        }
    };
    summary["status_code"] = json!(status);
    summary["rejected"] = json!(false);
    append_jsonl(&s.events.join("telegram-events.jsonl"), summary)?;
    write_http(
        &mut io,
        status,
        "application/json",
        serde_json::to_string(&json!({"ok":true,"result":result}))?.as_bytes(),
    )
}

fn http_provider(cli: &Cli) -> Result<()> {
    let port = cli.port.unwrap_or(443);
    let cfg = tls_config(&cli.tls_cert, &cli.tls_key)?;
    fs::create_dir_all(&cli.event_dir)?;
    append_jsonl(
        &cli.event_dir.join("http-provider-events.jsonl"),
        json!({"kind":"fixture_started","fixture":"http-providers","port":port}),
    )?;
    for incoming in TcpListener::bind(("0.0.0.0", port))?.incoming() {
        let tcp = incoming?;
        let c = Arc::clone(&cfg);
        let control = cli.control_dir.clone();
        let events = cli.event_dir.clone();
        thread::spawn(move || {
            if let Err(e) = http_conn(tcp, c, &control, &events) {
                eprintln!("http fixture: {e:#}")
            }
        });
    }
    Ok(())
}
fn http_conn(tcp: TcpStream, cfg: Arc<ServerConfig>, control: &Path, events: &Path) -> Result<()> {
    let conn = ServerConnection::new(cfg)?;
    let mut io = StreamOwned::new(conn, tcp);
    let req = read_http(&mut io)?;
    let host = req
        .headers
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_owned();
    let path = req.path.split('?').next().unwrap_or("/").to_owned();
    let root = read_json(&control.join("http.json"));
    let key = format!("{host}{path}");
    let cfg = root
        .get(&key)
        .or_else(|| root.get(&path))
        .or_else(|| root.get("default"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    sleep_mode(&cfg);
    let mode = cfg.get("mode").and_then(Value::as_str).unwrap_or("success");
    let mut status = 200u16;
    let mut body = match (host.as_str(), path.as_str()) {
        ("api.kaspa.org", "/info/price") => json!({"price":0.123456}),
        ("api.kaspa.org", "/info/marketcap") => json!({"marketcap":1234567890.0}),
        ("api.kaspa.org", "/info/fee-estimate") => {
            json!({"priorityBucket":{"feerate":3.0},"normalBuckets":[{"feerate":2.0}],"lowBuckets":[{"feerate":1.0}]})
        }
        ("api.coingecko.com", "/api/v3/simple/price") => {
            json!({"kaspa":{"usd":0.123456,"usd_market_cap":1234567890.0}})
        }
        ("api.coingecko.com", "/api/v3/coins/kaspa/market_chart/range") => {
            json!({"prices":[[unix_millis().saturating_sub(86_400_000),0.12],[unix_millis(),0.123456]]})
        }
        _ => {
            status = 404;
            json!({"error":"unsupported synthetic provider path"})
        }
    };
    match mode {
        "4xx" => {
            status = 400;
            body = json!({"error":"synthetic 4xx"})
        }
        "5xx" => {
            status = 503;
            body = json!({"error":"synthetic 5xx"})
        }
        "rate_limit" => {
            status = 429;
            body = json!({"error":"rate limited","retry_after":cfg.get("retry_after").and_then(Value::as_u64).unwrap_or(1)})
        }
        "invalid" => {
            body = if path == "/info/fee-estimate" {
                json!({"priorityBucket":{"feerate":-3.0},"normalBuckets":[],"lowBuckets":[]})
            } else if path == "/info/price" {
                json!({"price":-1})
            } else if path == "/info/marketcap" {
                json!({"marketcap":0})
            } else {
                json!({"kaspa":{"usd":-1,"usd_market_cap":0}})
            }
        }
        "missing" => body = json!({}),
        _ => {}
    }
    append_jsonl(
        &events.join("http-provider-events.jsonl"),
        json!({"kind":"http_provider_request","host":host,"path":path,"mode":mode,"status":status}),
    )?;
    if mode == "malformed" {
        write_http(&mut io, status, "application/json", b"{not-json")
    } else {
        write_http(
            &mut io,
            status,
            "application/json",
            serde_json::to_string(&body)?.as_bytes(),
        )
    }
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .ok()
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}
fn ws_accept(key: &str) -> String {
    let mut h = Sha1::new();
    h.update(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes());
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}
fn ws_send(stream: &mut TcpStream, payload: &[u8], opcode: u8) -> Result<()> {
    let mut head = vec![0x80 | opcode];
    let n = payload.len();
    if n < 126 {
        head.push(n as u8)
    } else if n <= 65535 {
        head.push(126);
        head.extend_from_slice(&(n as u16).to_be_bytes())
    } else {
        head.push(127);
        head.extend_from_slice(&(n as u64).to_be_bytes())
    }
    stream.write_all(&head)?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}
fn ws_read(stream: &mut TcpStream) -> Result<(u8, Vec<u8>)> {
    let mut h = [0u8; 2];
    stream.read_exact(&mut h)?;
    let opcode = h[0] & 0xf;
    let mut n = (h[1] & 0x7f) as u64;
    if n == 126 {
        let mut b = [0; 2];
        stream.read_exact(&mut b)?;
        n = u16::from_be_bytes(b) as u64
    } else if n == 127 {
        let mut b = [0; 8];
        stream.read_exact(&mut b)?;
        n = u64::from_be_bytes(b)
    }
    ensure!(n <= 1024 * 1024, "oversized frame");
    let mut mask = [0u8; 4];
    let masked = h[1] & 0x80 != 0;
    if masked {
        stream.read_exact(&mut mask)?
    }
    let mut p = vec![0u8; n as usize];
    stream.read_exact(&mut p)?;
    if masked {
        for (i, b) in p.iter_mut().enumerate() {
            *b ^= mask[i % 4]
        }
    }
    Ok((opcode, p))
}
fn kaspa(cli: &Cli) -> Result<()> {
    let port = cli.port.unwrap_or(17110);
    fs::create_dir_all(&cli.event_dir)?;
    append_jsonl(
        &cli.event_dir.join("kaspa-events.jsonl"),
        json!({"kind":"fixture_started","protocol":"workflow-rpc-0.18.0/pinned-cfafeb4","port":port}),
    )?;
    for incoming in TcpListener::bind(("0.0.0.0", port))?.incoming() {
        let s = incoming?;
        let c = cli.control_dir.clone();
        let e = cli.event_dir.clone();
        thread::spawn(move || {
            if let Err(x) = kaspa_conn(s, &c, &e) {
                eprintln!("kaspa fixture: {x:#}")
            }
        });
    }
    Ok(())
}
fn kaspa_conn(mut stream: TcpStream, control: &Path, events: &Path) -> Result<()> {
    let mut peek = [0u8; 4096];
    let n = stream.peek(&mut peek)?;
    let header = String::from_utf8_lossy(&peek[..n]).to_string();
    if !header.to_ascii_lowercase().contains("upgrade: websocket") {
        let req = read_http(&mut stream)?;
        let _ = req.method;
        return write_http(&mut stream, 200, "text/plain", b"local kaspa fixture\n");
    }
    let req = read_http(&mut stream)?;
    let key = req
        .headers
        .get("sec-websocket-key")
        .and_then(Value::as_str)
        .unwrap_or("");
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        ws_accept(key)
    )?;
    stream.flush()?;
    append_jsonl(
        &events.join("kaspa-events.jsonl"),
        json!({"kind":"websocket_connected"}),
    )?;
    loop {
        let (opcode, payload) = match ws_read(&mut stream) {
            Ok(x) => x,
            Err(_) => break,
        };
        if opcode == 8 {
            ws_send(&mut stream, &payload, 8)?;
            break;
        }
        if opcode == 9 {
            ws_send(&mut stream, &payload, 10)?;
            continue;
        }
        if opcode != 1 {
            continue;
        }
        let req: Value = serde_json::from_slice(&payload)?;
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let root = read_json(&control.join("kaspa.json"));
        let cfg = root
            .get(method)
            .or_else(|| root.get("default"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        sleep_mode(&cfg);
        let mode = cfg.get("mode").and_then(Value::as_str).unwrap_or("success");
        append_jsonl(
            &events.join("kaspa-events.jsonl"),
            json!({"kind":"kaspa_request","method":method,"mode":mode,"params":req.get("params"),"supported":true}),
        )?;
        if mode == "disconnect" {
            break;
        }
        if mode == "malformed" {
            if req.get("id").is_some() {
                ws_send(&mut stream, b"{not-json", 1)?
            }
            continue;
        }
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let result = result_for(method, control);
        let response = match result {
            Some(mut v) => {
                if mode == "invalid" {
                    v = if method == "getSyncStatus" {
                        json!({"isSynced":"invalid"})
                    } else if method == "getServerInfo" {
                        json!({"networkId":"invalid","isSynced":false})
                    } else {
                        json!({"syntheticInvalid":true})
                    }
                }
                if ["error", "5xx", "4xx", "rate_limit"].contains(&mode) {
                    json!({"id":id,"error":{"code":if mode=="rate_limit"{429}else{1},"message":format!("synthetic {mode}")}})
                } else {
                    json!({"id":id,"params":v})
                }
            }
            None => {
                json!({"id":id,"error":{"code":1,"message":"Unsupported local fixture method","data":{"method":method}}})
            }
        };
        if req.get("id").is_some() {
            ws_send(&mut stream, serde_json::to_string(&response)?.as_bytes(), 1)?
        }
    }
    append_jsonl(
        &events.join("kaspa-events.jsonl"),
        json!({"kind":"websocket_closed"}),
    )?;
    Ok(())
}
fn result_for(method: &str, control: &Path) -> Option<Value> {
    let score = 1_000_000u64
        + SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    let zero = "00".repeat(32);
    Some(match method {
        "getServerInfo" => {
            json!({"rpcApiVersion":1,"rpcApiRevision":0,"serverVersion":"local-fixture-cfafeb4","networkId":"mainnet","hasUtxoIndex":true,"isSynced":true,"virtualDaaScore":score})
        }
        "getSyncStatus" => json!({"isSynced":true}),
        "getBlockDagInfo" => {
            json!({"network":"mainnet","blockCount":1,"headerCount":1,"tipHashes":[zero],"difficulty":1.0,"pastMedianTime":unix_millis(),"virtualParentHashes":["00".repeat(32)],"pruningPointHash":"00".repeat(32),"virtualDaaScore":score,"sink":"00".repeat(32)})
        }
        "getCoinSupply" => {
            json!({"maxSompi":2870400000000000000u64,"circulatingSompi":2000000000000000000u64})
        }
        "getUtxosByAddresses" => {
            json!({"entries":read_json(&control.join("kaspa.json")).get("utxos").and_then(Value::as_array).cloned().unwrap_or_default()})
        }
        "getConnectedPeerInfo" => json!({"peerInfo":[]}),
        "getBlockCount" => json!({"blockCount":1,"headerCount":1}),
        "getSink" => json!({"sink":"00".repeat(32)}),
        "getCurrentNetwork" => json!({"currentNetwork":"mainnet"}),
        "estimateNetworkHashesPerSecond" => json!({"networkHashesPerSecond":1000000}),
        "subscribe" => json!({"id":1}),
        "unsubscribe" | "notifyVirtualDaaScoreChanged" | "ping" => json!({}),
        _ => return None,
    })
}

fn probe(cli: &Cli) -> Result<()> {
    let url = cli
        .url
        .as_deref()
        .context("--url is required for probe mode")?;
    let (scheme, rest) = url
        .split_once("://")
        .context("probe URL must include scheme")?;
    let (authority, path) = rest
        .split_once('/')
        .map(|(a, p)| (a, format!("/{p}")))
        .unwrap_or((rest, "/".to_owned()));
    let (host, port) = authority
        .rsplit_once(':')
        .and_then(|(h, p)| p.parse::<u16>().ok().map(|p| (h, p)))
        .unwrap_or((authority, if scheme == "https" { 443 } else { 80 }));
    let tcp = TcpStream::connect((host, port)).with_context(|| format!("connect {host}:{port}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(5)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(5)))?;
    if scheme == "https" {
        let ca = cli
            .ca
            .as_deref()
            .context("--ca is required for HTTPS probe")?;
        let mut roots = RootCertStore::empty();
        let mut reader = BufReader::new(fs::File::open(ca)?);
        for cert in rustls_pemfile::certs(&mut reader) {
            roots.add(cert?)?;
        }
        let cfg = Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        let server_name = rustls::pki_types::ServerName::try_from(host.to_owned())
            .context("invalid TLS server name")?;
        let conn = ClientConnection::new(cfg, server_name)?;
        let mut io = StreamOwned::new(conn, tcp);
        write!(
            io,
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )?;
        io.flush()?;
        let mut out = Vec::new();
        io.read_to_end(&mut out)?;
        print_probe(&out)
    } else if scheme == "http" {
        let mut io = tcp;
        write!(
            io,
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )?;
        io.flush()?;
        let mut out = Vec::new();
        io.read_to_end(&mut out)?;
        print_probe(&out)
    } else {
        bail!("unsupported probe scheme: {scheme}")
    }
}
fn print_probe(raw: &[u8]) -> Result<()> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .context("invalid HTTP response")?;
    let status = head.lines().next().context("missing status line")?;
    let code = status
        .split_whitespace()
        .nth(1)
        .context("missing status code")?;
    println!("STATUS={code}");
    print!("{body}");
    Ok(())
}

fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("rustls provider already installed"))
        .ok();
    let cli = Cli::parse();
    match cli.mode {
        Mode::Telegram => telegram(&cli),
        Mode::Http => http_provider(&cli),
        Mode::Kaspa => kaspa(&cli),
        Mode::Probe => probe(&cli),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn websocket_accept_matches_rfc6455() {
        assert_eq!(
            ws_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
    #[test]
    fn url_decode_form() {
        assert_eq!(url_decode("hello+world%21"), "hello world!");
    }
}
