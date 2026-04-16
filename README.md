```markdown
# 🦀 Kaspa Pulse: The Ultimate Enterprise Miner's Companion

![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg?style=flat-square)
![Kaspa](https://img.shields.io/badge/Kaspa-Network-70D4CB.svg?style=flat-square)
![Database](https://img.shields.io/badge/Database-SQLite-blue.svg?style=flat-square)
![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)
![AI-Powered](https://img.shields.io/badge/AI-Gemini%202.5%20Flash-purple.svg?style=flat-square)
![Zero-Warnings](https://img.shields.io/badge/Standard-Zero%20Warnings-success.svg?style=flat-square)

---

## 🚀 Overview

**Kaspa Pulse** (formerly Kaspa Solo) is an ultra-high-performance, enterprise-grade Telegram bot engineered entirely in Rust.

Built for **Kaspa Solo Miners** and **Full Node Operators**, it delivers:

* ⚡ Zero-latency notifications
* 🔐 Maximum privacy (no public APIs)
* 🧠 AI-powered intelligence
* 🔬 Deep GHOSTDAG blockchain forensics

It connects directly to your node via **wRPC WebSocket**, ensuring raw, unindexed, real-time blockchain data without relying on third-party block explorers.

---

## ✨ Core Features

### 🧠 Advanced AI Intelligence (Universal Standard API)
* **Voice-to-Text Analytics:** Send a voice note (OGG) directly to the bot. It automatically transcribes the audio and feeds it to the AI for contextual answers.
* **Context-Aware Responses:** The AI knows your live wallet balance, current network DAA score, Kaspa price, and difficulty in real-time.
* **Universal API Support:** Seamlessly integrated with OpenAI-standard APIs (supports Groq's Llama 3, OpenAI, or Google Gemini via API overrides).
* **Enterprise Retry Logic:** Built-in exponential backoff handles 429 (Rate Limit) and 503 (Service Unavailable) errors silently.

### 🛡️ Smart Node Safety Protocol (Anti-Ban)
* Built-in `is_local_node()` heuristic engine.
* Automatically detects if you are connected to a `localhost` (127.0.0.1) or an external Public Node (like `kaspadns`).
* **Protects Public Nodes:** Disables CPU-intensive historical reverse-syncs when on a public node to prevent your IP from being banned, while keeping Live UTXO tracking active.

### 🎯 Deterministic Block Detection
* Reverse DAG traversal logic.
* Byte-level payload scanning to verify actual block ownership.
* 100% accurate reward attribution.
* Zero explorer dependency (completely decentralized).

### 🔬 Mining Forensics & Rich Logging
* Extracts block **Nonce**.
* Decodes **Worker ID** directly from the block payload.
* Identifies exact mining source and provides rich, beautifully formatted Telegram alerts (`[RECOVERY SUCCESS]` and `[LIVE BLOCK]`).

### 🕒 Smart UTXO Processing
* Parallel processing via `tokio::task::JoinSet`.
* Chronological sorting using `block_time_ms`.
* Perfect notification ordering, ensuring no dropped or out-of-sync messages.

### ⛏️ Hashrate Estimation
* 1H / 24H / 7D live analysis.
* Accurately calculates your hash power based on *real* mined rewards and the current global network difficulty.

### 🗄️ Storage & Modular Engine
* SQLite (ACID compliant) database for crash-safe state management.
* Fast startup with immediate memory recovery.
* **Modular Enterprise Architecture:** Codebase strictly divided into `ai/`, `handlers/`, and `workers/` for Zero-Warning compilation and extreme scalability.

---

## 📖 How to Use Kaspa Pulse

Once the bot is deployed and connected to your Telegram, using it is incredibly intuitive:

1. **Initialize the Bot:** Send `/start` to your bot. It will recognize if you are the Admin (based on your `.env` configuration) and grant you the appropriate access tier.
2. **Smart Auto-Add Wallets:** You don't even need commands! Just paste a valid Kaspa address (e.g., `kaspa:q...` or just the `q...` part) into the chat. The bot uses heuristic text detection to automatically add it to the monitoring pool.
3. **Interactive Menus:** Click the inline buttons provided by `/start` to view your balances, network health, or mined blocks without typing anything.
4. **Chat with AI:** Simply type a question like *"What is my current hashrate?"* or *"Explain the GHOSTDAG consensus"* and the AI will reply using your live node data. You can also send voice messages for hands-free querying!

---

## 📱 Commands

### 📌 Public Commands

| Command             | Description                                     |
| ------------------- | ----------------------------------------------- |
| `/start`            | Initialize bot and show the interactive menu    |
| `/help`             | Show command guide                              |
| `/add <address>`    | Track a specific wallet address                 |
| `/remove <address>` | Stop tracking a wallet                          |
| `/list`             | Show all currently tracked wallets              |
| `/balance`          | Show live balance and UTXO count                |
| `/blocks`           | Count mined blocks and lifetime value           |
| `/miner`            | Estimate solo hashrate (1H / 24H)               |
| `/network`          | Check Node sync status, peers, and DAG info     |
| `/dag`              | Quick overview of BlockDAG headers and blocks   |
| `/price`            | Current KAS price (via CoinGecko)               |
| `/market`           | Current KAS market capitalization               |
| `/supply`           | Circulating vs Max Supply percentages           |
| `/fees`             | Real-time Mempool fee estimation                |
| `/donate`           | Support the development of Kaspa Pulse          |

---

### 👑 Admin Enterprise Commands (Restricted)

| Command              | Description                                          |
| -------------------- | ---------------------------------------------------- |
| `/stats`             | View global bot analytics and user reports           |
| `/sys`               | Hardware diagnostics (CPU, RAM, Uptime, Disk space)  |
| `/logs`              | Fetch the last 25 lines of `bot.log` remotely        |
| `/broadcast <msg>`   | Send a global message to all users tracking a wallet |
| `/pause` / `/resume` | Suspend or resume the background UTXO monitoring     |
| `/restart`           | Safely restart the bot binary remotely               |
| `/learn <text>`      | Manually inject new Kaspa facts into the AI's Vector DB |
| `/autolearn`         | Force the AI to scrape official Kaspa RSS feeds      |
| `/sync`              | Trigger a manual historical BlockDAG reverse-scan    |

---

## ⚙️ Prerequisites

### 🔧 Build Tools

* **Windows**
```bash
winget install cmake
```

* **Linux**
```bash
sudo apt update && sudo apt install cmake build-essential pkg-config libssl-dev
```

---

## 🔐 Environment Setup

Create a `.env` file in the root directory of the project:

```env
# Telegram Bot Configuration
BOT_TOKEN=your_telegram_bot_token_here
ADMIN_ID=your_telegram_user_id

# Kaspa Node Connection (Local or Public)
# NOTE: If using a public node, historical sync is disabled automatically for safety.
WS_URL=ws://127.0.0.1:18110

# AI Configuration (Universal OpenAI Standard API)
# Get a free key from: [https://console.groq.com/keys](https://console.groq.com/keys) or use Google/OpenAI
AI_API_KEY=your_ai_api_key_here
AI_BASE_URL=[https://api.groq.com/openai/v1](https://api.groq.com/openai/v1)
AI_CHAT_MODEL=llama-3.3-70b-versatile
AI_AUDIO_MODEL=whisper-large-v3

# System Logging
RUST_LOG=info,kaspa_solo=debug
```

---

## 🛠️ Deployment

### 📦 Clone the Repository

```bash
git clone [https://github.com/KaspaPulse/kaspa-gemini-intelligence.git](https://github.com/KaspaPulse/kaspa-gemini-intelligence.git)
cd kaspa-gemini-intelligence
```

---

### 🐧 Linux Deployment

#### 1. Install Rust & Dependencies

```bash
sudo apt update && sudo apt install -y curl build-essential pkg-config libssl-dev cmake
curl --proto '=https' --tlsv1.2 -sSf [https://sh.rustup.rs](https://sh.rustup.rs) | sh
source $HOME/.cargo/env
```

#### 2. Build the Enterprise Binary

```bash
cargo build --release
```

#### 3. Run as a Systemd Service (Recommended)

```bash
sudo nano /etc/systemd/system/kaspa-pulse.service
```

Paste the following (change `your_username` accordingly):

```ini
[Unit]
Description=Kaspa Pulse Enterprise Bot
After=network.target

[Service]
User=your_username
WorkingDirectory=/home/your_username/kaspa-gemini-intelligence
ExecStart=/home/your_username/kaspa-gemini-intelligence/target/release/kaspa-solo
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable kaspa-pulse
sudo systemctl start kaspa-pulse
```

---

### 🪟 Windows Deployment

#### 1. Build the Binary

```powershell
cargo build --release
```

#### 2. Run as a Background Service (NSSM)

Download [NSSM](http://nssm.cc/), extract it, and run the following in an Administrator Command Prompt:

```cmd
nssm install KaspaPulseBot
```
*(Point the Application Path to your `target\release\kaspa-solo.exe` and the Startup Directory to the root folder where `.env` is located).*

Start the service:
```cmd
nssm start KaspaPulseBot
```

---

## 🤝 Contributing

We welcome contributions! Please follow the standard Git Flow:

```bash
git checkout -b feature/new-feature
# Write awesome code
git commit -m "feat: add feature X"
git push origin feature/new-feature
```
Then, open a Pull Request on GitHub.

---

## 💖 Support the Developer

If Kaspa Pulse has helped you track your solo mining rewards or manage your node, consider supporting the development!

**Kaspa (KAS) Donation Address:**
```text
kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g
```

---

## 📜 License

This project is licensed under the **MIT License**. See the `LICENSE` file for details.

---

## 🧠 Final Note

Built with precision, engineered with Rust, and designed for the true Kaspa ecosystem pioneers. Happy Solo Mining! ⛏️
```