# 🦀 Kaspa Pulse: The Ultimate Enterprise Miner's Companion

![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg?style=flat-square)
![Kaspa](https://img.shields.io/badge/Kaspa-Network-70D4CB.svg?style=flat-square)
![Database](https://img.shields.io/badge/Database-PostgreSQL-336791.svg?style=flat-square)
![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)
![AI-Powered](https://img.shields.io/badge/AI-Llama3%20%7C%20OpenAI-purple.svg?style=flat-square)
![Architecture](https://img.shields.io/badge/Architecture-Parallel%20Streaming-success.svg?style=flat-square)
![State Management](https://img.shields.io/badge/State-DashMap%20RAM-blue.svg?style=flat-square)
![Standard](https://img.shields.io/badge/Standard-Zero%20Warnings-success.svg?style=flat-square)

---

## 🚀 Overview

**Kaspa Pulse** (formerly Kaspa Solo) is an ultra-high-performance, enterprise-grade Telegram bot engineered entirely in Rust.

Built for **Kaspa Solo Miners** and **Full Node Operators**, it delivers:

* ⚡ Zero-latency notifications via direct wRPC.
* 🔐 Maximum privacy (no public APIs or external explorers).
* 🧠 RAG AI-powered intelligence (Vector Search & Voice-to-Text).
* 🔬 Deep GHOSTDAG blockchain forensics (Nonce & Worker extraction).
* 🐘 High-Performance PostgreSQL State Management.
* 🛡️ Anti-Flood, Rate Limited & Prompt-Injection Hardened.
* ⚙️ **Dynamic Enterprise Control Panel** (Zero-Downtime Configuration).

It connects directly to your node via **wRPC WebSocket**, ensuring raw, unindexed, real-time blockchain data.

---

## ✨ Core Features

### 🧠 Advanced AI Intelligence (Universal Standard API)
* **Voice-to-Text Analytics:** Send a voice note (OGG) directly to the bot. It transcribes the audio and feeds it to the AI for contextual answers (Whisper V3).
* **Context-Aware RAG:** The AI knows your live wallet balance, network DAA score, price, and searches a localized Vector Database (`pgvector`) for precise Kaspa knowledge.
* **Universal API Support:** Seamlessly integrated with OpenAI-standard APIs (supports Groq, OpenAI, or compatible endpoints).
* **Enterprise Retry Logic:** Built-in exponential backoff handles 429 (Rate Limit) errors silently without crashing.

### ⚙️ Dynamic Enterprise Control Panel
Manage the entire bot's behavior in real-time directly from Telegram (Zero Downtime, no rebuild required):
* Toggle **Maintenance Mode** or **Private Access**.
* Enable/Disable **AI Vectorizer**, **RSS Worker**, and **Memory Cleaner**.
* Switch **AI Providers** or adjust **Mining Confirmations** instantly via `/toggle` commands.

### 🛡️ Smart Node Safety Protocol (Anti-Ban)
* Built-in `is_local_node()` heuristic engine.
* Automatically detects if you are connected to a `localhost` (127.0.0.1) or an external Public Node.
* **Protects Public Nodes:** Disables CPU-intensive historical reverse-syncs when on a public node to prevent your IP from being banned, while keeping Live UTXO tracking active.

### 🎯 Deterministic Block Detection & Forensics
* Reverse DAG traversal logic.
* Byte-level payload scanning to verify actual block ownership.
* 100% accurate reward attribution.
* Extracts block **Nonce** and decodes **Worker ID** directly from the block payload.

### 🕒 Smart UTXO Processing
* Parallel processing via `tokio::task::JoinSet`.
* Chronological sorting using `block_time_ms`.
* Perfect notification ordering, ensuring no dropped or out-of-sync messages.

### ⛏️ Hashrate Estimation
* 1H / 24H / 7D live analysis.
* Accurately calculates your hash power based on *real* mined rewards and the current global network difficulty.

### 🗄️ Storage & Modular Engine
* **PostgreSQL** (`sqlx`) database for crash-safe state management and `pgvector` indexing.
* **DashMap** for ultra-low latency RAM state.
* **Modular Architecture:** Codebase strictly divided into `ai/`, `handlers/`, `workers/`, and `services/` for extreme scalability.

---

## 📖 How to Use Kaspa Pulse

Once deployed:

1. **Initialize:** Send `/start` to the bot. It will recognize if you are the Admin and grant the appropriate access tier.
2. **Smart Auto-Add:** Paste any valid Kaspa address (e.g., `kaspa:q...`) into the chat. Heuristic detection automatically adds it to the monitoring pool.
3. **Interactive Menus:** Click the inline buttons to view balances, network health, or mined blocks without typing.
4. **Chat with AI:** Type *"What is my current hashrate?"* or send a voice message. The AI replies using your live node data.

---

## 📱 Commands

### 📌 Public Commands

| Command | Description |
|---|---|
| `/start` | Initialize bot and show the interactive menu |
| `/help` | Show command guide |
| `/add <address>` | Track a specific wallet address |
| `/remove <address>` | Stop tracking a wallet |
| `/list` | Show all currently tracked wallets |
| `/balance` | Show live balance and UTXO count |
| `/blocks` | Count mined blocks and lifetime value |
| `/miner` | Estimate solo hashrate (1H / 24H) |
| `/network` | Check Node sync status, peers, and DAG info |
| `/dag` | Quick overview of BlockDAG headers and blocks |
| `/price` | Current KAS price (via CoinGecko) |
| `/market` | Current KAS market capitalization |
| `/supply` | Circulating vs Max Supply percentages |
| `/fees` | Real-time Mempool fee estimation |
| `/donate` | Support the development of Kaspa Pulse |

---

### 👑 Admin Enterprise Commands (Restricted)

| Command | Description |
|---|---|
| `/settings` | Open the Dynamic Enterprise Control Panel |
| `/toggle <FLAG>` | Toggle a specific configuration flag (e.g., `MAINTENANCE_MODE`) |
| `/stats` | View global bot analytics and user reports |
| `/sys` | Hardware diagnostics (CPU, RAM, Uptime, Disk space) |
| `/logs` | Fetch the last 25 lines of `bot.log` remotely |
| `/broadcast <msg>` | Send a global message to all users |
| `/pause` / `/resume` | Suspend or resume the background UTXO monitoring |
| `/restart` | Safely restart the bot binary remotely |
| `/learn <text>` | Manually inject new Kaspa facts into the AI's Vector DB |
| `/autolearn` | Force the AI to scrape official Kaspa RSS feeds |
| `/sync` | Trigger a manual historical BlockDAG reverse-scan |

---

## ⚙️ Prerequisites

### 🔧 Build Tools & Database

* **Rust:** `v1.70+`
* **Database:** PostgreSQL `v15+` with `pgvector` extension enabled.
* **OS:** Linux (Ubuntu 22.04/24.04 recommended) or Windows.

```
# Ubuntu Dependencies
sudo apt update && sudo apt install -y cmake build-essential pkg-config libssl-dev postgresql postgresql-contrib

---

🔐 Environment Setup
Create a .env file in the root directory:

Code snippet
# 🤖 TELEGRAM BOT CONFIGURATION
BOT_TOKEN=your_telegram_bot_token_here
ADMIN_ID=your_telegram_user_id

# ⚙️ NODE & INFRASTRUCTURE
WS_URL=ws://127.0.0.1:18110
DATABASE_URL=postgres://user:password@127.0.0.1:5432/kaspa_db
RUST_LOG=info,kaspa_solo=debug

# 🧠 AI CONFIGURATION
AI_API_KEY=your_groq_or_openai_key
AI_BASE_URL=https://api.groq.com/openai/v1
AI_CHAT_MODEL=llama-3.3-70b-versatile
AI_AUDIO_MODEL=whisper-large-v3

# 🛡️ ACCESS & SECURITY (Dynamic Flags)
MAINTENANCE_MODE=false
ALLOW_PUBLIC_USERS=true
ENABLE_RSS_WORKER=true
ENABLE_MEMORY_CLEANER=true
ENABLE_LIVE_SYNC=true
ENABLE_AI_VECTORIZER=false

---

# 🛠️ Deployment (Linux)
1. Setup PostgreSQL Database
Bash
sudo -u postgres psql
CREATE DATABASE kaspa_db;
CREATE USER kaspa_admin WITH PASSWORD 'super_secret_password';
GRANT ALL PRIVILEGES ON DATABASE kaspa_db TO kaspa_admin;
\c kaspa_db
CREATE EXTENSION vector;
\q

---

# 2. Build the Enterprise Binary
Bash
git clone https://github.com/KaspaPulse/kaspa-gemini-intelligence.git
cd kaspa-gemini-intelligence
cargo build --release

---

# 3. Run as a Systemd Service
Bash
sudo nano /etc/systemd/system/kaspa-pulse.service
Ini, TOML

[Unit]
Description=Kaspa Pulse Enterprise Bot
After=network.target postgresql.service

[Service]
User=your_username
WorkingDirectory=/home/your_username/kaspa-gemini-intelligence
ExecStart=/home/your_username/kaspa-gemini-intelligence/target/release/kaspa-solo
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
Bash

sudo systemctl daemon-reload
sudo systemctl enable kaspa-pulse
sudo systemctl start kaspa-pulse

---

# 💖 Support the Developer
If Kaspa Pulse has helped you track your solo mining rewards or manage your node, consider supporting the development!

Kaspa (KAS) Donation Address:

Plaintext
kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g

---

# 📜 License
This project is licensed under the MIT License.

---

# 🧠 Final Note
Built with precision, engineered with Rust, and designed for the true Kaspa ecosystem pioneers. Happy Solo Mining! ⛏️