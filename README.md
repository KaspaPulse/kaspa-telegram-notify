```markdown
# 🦀 Kaspa Pulse: The Ultimate Enterprise Miner's Companion

![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg?style=flat-square)
![Kaspa](https://img.shields.io/badge/Kaspa-Network-70D4CB.svg?style=flat-square)
![Database](https://img.shields.io/badge/Database-SQLite-blue.svg?style=flat-square)
![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)
![AI-Powered](https://img.shields.io/badge/AI-Gemini%202.5%20Flash-purple.svg?style=flat-square)
![Zero-Warnings](https://img.shields.io/badge/Standard-Zero%20Warnings-success.svg?style=flat-square)

**Kaspa Pulse** (formerly Kaspa Solo) is an ultra-high-performance, enterprise-grade Telegram bot engineered entirely in Rust. Designed specifically for **Kaspa Solo Miners** and **Full Node Operators**, it delivers cryptographic precision, zero-latency notifications, and deep GHOSTDAG forensics.

By establishing a direct WebSocket (wRPC) connection to your local node, the bot completely bypasses third-party public APIs, ensuring maximum privacy, decentralized resilience, and access to raw, unindexed blockchain data.

---

## 🚀 Enterprise Architecture & Features

This engine has been completely re-architected to meet **Zero Warnings** production standards, featuring a powerful cloud-native backend combined with relentless local cryptography:

### 1. 🧠 Cloud-Native Multimodal AI (Gemini 2.5 Flash)
* **💬 Conversational Intent:** Chat with the bot naturally. It analyzes your tracked wallets, node DAA score, and live network difficulty to provide highly contextual answers.
* **🎙️ Zero-Latency Voice Processing:** Send raw voice messages (OGG) directly to Gemini via Base64 encoding. The bot understands audio and responds instantly.
* **🔄 Exponential Backoff Protocol:** Engineered with a resilient retry mechanism to survive API rate limits (HTTP 503/429) during peak Google Cloud traffic.

### 2. 🎯 Deterministic Deep Payload Scan
Standard block explorers merely link rewards to the "Accepting Block." Our proprietary algorithm conducts a reverse DAG traversal, performing a cryptographic byte-by-byte scan of every Blue Block's payload against your wallet's `ScriptPublicKey`. This guarantees **100% mathematical certainty** of the exact block your hardware mined—eliminating any reliance on index guessing.

### 3. 🔬 Cryptographic Forensics (Nonce & Worker ID)
Whenever a block is discovered, the bot acts as a forensic analyzer:
* **Nonce Extraction:** Parses the block header to retrieve the exact mathematical Nonce that solved the hash.
* **Worker Decoding:** Decrypts and sanitizes the dead bytes within the coinbase payload to extract your exact mining rig/worker name (e.g., `1.1.0/RK-Stratum/KaspaPulse`).

### 4. 🕒 Temporal UTXO Sorting Engine
When mining solo, blocks are often found in rapid succession. The bot utilizes `tokio::task::JoinSet` to process multiple UTXO rewards concurrently, sorts them strictly by their exact `block_time_ms`, and dispatches Telegram notifications in perfect chronological order.

### 5. ⛏️ Real-Time Hashrate Estimation
Monitor your mining fleet's performance natively. The bot analyzes your accepted `Coinbase UTXOs` over specific time windows to calculate an accurate local hashrate (1H, 24H, 7D).

### 6. 🗄️ ACID-Compliant SQLite Storage
Thread-safe state management, instant boot times, and protection against data corruption during unexpected server shutdowns.

---

## 📱 Command Reference

### 📌 Public Commands

| Command | Description |
| :--- | :--- |
| `/start` | Initializes the bot and renders the interactive UI keyboard. |
| `/help` | Show the ultimate guide and features. |
| `/add <address>` | Subscribe to real-time reward tracking for a wallet. |
| `/remove <address>`| Unsubscribe a wallet from tracking. |
| `/list` | List tracked wallets. |
| `/balance` | Check Live Balance & UTXOs. |
| `/blocks` | Count your unspent mined blocks. |
| `/miner` | Estimate your solo-mining hashrate. |
| `/network` | Show full node and network stats. |
| `/dag` | Show BlockDAG details. |
| `/price` | Check KAS Price. |
| `/market` | Check Market Cap. |
| `/supply` | Check Supply. |
| `/fees` | Check Mempool Fees. |
| `/donate` | Support the Developer. |

### 👑 Admin Diagnostics (Secured Scope)

These commands are cryptographically restricted to the `ADMIN_ID` specified in the `.env` file.

| Command | Description |
| :--- | :--- |
| `/stats` | Admin Analytics: Active users, tracked wallets, and Ping. |
| `/sys` | Hardware diagnostics: Monitor RAM utilization and thread health. |
| `/logs` | Fetch the deep telemetry `bot.log` file directly in Telegram. |
| `/broadcast <msg>`| Push a global announcement to all subscribed users. |
| `/pause` / `/resume`| Temporarily suspend or resume the background UTXO monitoring engine. |
| `/restart` | Gracefully terminate and reboot the Rust process. |
| `/learn <text>` | Admin Command: Teach AI new Kaspa knowledge dynamically. |
| `/autolearn` | Auto-fetch latest official Kaspa news via RSS into the knowledge base. |

---

## ⚙️ Prerequisites & Environment Setup

To successfully compile and run Kaspa Pulse, your system must have the following external dependencies installed:

### 1. Mandatory Build Tools (C++ & CMake)
* **Windows:** `winget install cmake` (Ensure "Desktop development with C++" is selected in VS Build Tools).
* **Linux (Ubuntu/Debian):** `sudo apt update && sudo apt install cmake build-essential`

### 2. Environment Configuration (`.env`)
Create a `.env` file in the project's root directory:

```env
BOT_TOKEN=your_telegram_bot_token_here
ADMIN_ID=your_telegram_user_id
WS_URL=ws://127.0.0.1:18110
GEMINI_API_KEY=your_google_ai_studio_key_here
RUST_LOG=info,kaspa_solo=debug
```

---

## 🛠️ Deployment Guide

### 📦 Step 0: Clone the Repository
```bash
git clone https://github.com/KaspaPulse/kaspa-gemini-intelligence.git
cd kaspa-gemini-intelligence
```

### 🐧 Linux (Ubuntu/Debian) Deployment

**1. Install Dependencies & Rust:**
```bash
sudo apt update && sudo apt install -y curl build-essential pkg-config libssl-dev cmake
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

**2. Compile the Engine:**
```bash
cargo build --release
```

**3. Create a Persistent `systemd` Background Service:**
```bash
sudo nano /etc/systemd/system/kaspa-pulse.service
```

Paste the following configuration (replace `your_username` with your actual system user):

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

**Save and initialize the service:**
```bash
sudo systemctl daemon-reload
sudo systemctl enable kaspa-pulse
sudo systemctl start kaspa-pulse
```

### 🪟 Windows Server Deployment

**1. Compile the Engine:** Open `PowerShell` in the project directory:

```powershell
cargo build --release
```

**2. Create a Persistent Background Service via NSSM:**

* Download and extract NSSM.
* Open `Command Prompt` as **Administrator**, navigate to the `win64` NSSM directory, and run:

    ```cmd
    nssm install KaspaPulseBot
    ```

* In the GUI that appears:
    * **Path:** Browse and select `C:\path\to\target\release\kaspa-solo.exe`
    * **Directory:** Browse and select the project root.

* Click **Install service**, then start it:

    ```cmd
    nssm start KaspaPulseBot
    ```

---

## 🤝 Contributing

We embrace the open-source ethos. To contribute:
1. **Fork** the repository.
2. Create a **Feature Branch** (`git checkout -b feature/NextGenFeature`).
3. Commit your changes (`git commit -m 'feat: Add NextGenFeature'`).
4. Push to the branch (`git push origin feature/NextGenFeature`).
5. Open a **Pull Request**.

*(All PRs must pass `cargo fmt` and `cargo clippy --fix` to maintain the Zero Warnings standard).*

---

## 💖 Support the Project

Engineering a zero-warning, highly concurrent Rust engine requires countless hours of development and live-node testing. If this software has secured your operations or streamlined your node management, consider supporting the architect:

**Kaspa (KAS) Donation Address:**
`kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g`

*Built with absolute mathematical precision for the Kaspa ecosystem.*
```