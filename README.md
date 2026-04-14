# 🦀 Kaspa Solo: The Ultimate Enterprise Miner's Companion

![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg?style=flat-square)
![Kaspa](https://img.shields.io/badge/Kaspa-Network-70D4CB.svg?style=flat-square)
![Database](https://img.shields.io/badge/Database-SQLite-blue.svg?style=flat-square)
![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)
![AI-Powered](https://img.shields.io/badge/AI-Whisper%20%26%20Qwen-purple.svg?style=flat-square)

**Kaspa Solo** is an ultra-high-performance, enterprise-grade Telegram bot engineered entirely in Rust. Designed specifically for **Kaspa Solo Miners** and **Full Node Operators**, it delivers cryptographic precision, zero-latency notifications, and deep GHOSTDAG forensics.

By establishing a direct WebSocket (wRPC) connection to your local node, the bot completely bypasses third-party public APIs, ensuring maximum privacy, decentralized resilience, and access to raw, unindexed blockchain data.

---

## 🚀 Enterprise Architecture & Features

This bot is powered by a local AI engine (Whisper & Qwen 2.5), allowing for a seamless, human-like interaction.

* **💬 Text Interaction:** You can chat with the bot directly. Ask about network difficulty, hashrate, or your specific wallet balance in natural language, and it will respond with real-time data from your node.
* **🎙️ Voice Interaction:** Send a voice message (Voice Note) to the bot. It will transcribe your speech locally using **Whisper AI**, understand your intent, and respond to your query.
* **🔒 Absolute Privacy:** All AI processing (Speech-to-Text and LLM) happens **locally on your machine**. [cite_start]Your voice and data never leave your server to third-party AI providers.

### 1. 🎯 Deterministic Deep Payload Scan
Standard block explorers merely link rewards to the "Accepting Block." Our proprietary algorithm conducts a reverse DAG traversal, performing a cryptographic byte-by-byte scan of every Blue Block's payload against your wallet's `ScriptPublicKey`. This guarantees **100% mathematical certainty** of the exact block your hardware mined—eliminating any reliance on index guessing.

### 2. 🔬 Cryptographic Forensics (Nonce & Worker ID)
Whenever a block is discovered, the bot acts as a forensic analyzer:
* **Nonce Extraction:** Parses the block header to retrieve the exact mathematical Nonce that solved the hash.
* **Worker Decoding:** Decrypts and sanitizes the dead bytes within the coinbase payload to extract your exact mining rig/worker name (e.g., `1.1.0/RK-Stratum/KaspaPulse`).

### 3. ⛏️ Real-Time Hashrate Estimation
Monitor your mining fleet's performance natively. The bot analyzes your accepted `Coinbase UTXOs` over specific time windows to calculate an accurate local hashrate:
* **1-Hour (1H):** Immediate performance and drop-off monitoring.
* **24-Hour (24H):** Daily stability and yield analysis.
* **7-Day (7D):** Long-term operational efficiency.

### 4. 🗄️ ACID-Compliant SQLite Storage
Migrated from fragile flat JSON files to a robust `rusqlite` database engine. This ensures thread-safe state management, instant boot times, and protection against data corruption during unexpected server shutdowns.

### 5. 🛡️ Deep Telemetry & Anti-Spam Mitigation
* **Military-Grade Rate Limiting:** Prevents malicious or accidental spam from overloading the wRPC connection.
* **Asynchronous Tracing:** Implements a zero-warning, deep telemetry logging system recording execution times in milliseconds (`ms`) for seamless debugging and server auditing.

---

## 🛠️ Deployment Guide

### Prerequisites
1. **Rusty Kaspa Node:** A fully synced node with `utxoindex=true` enabled.
2. **Telegram Bot Token:** Generated via [@BotFather](https://t.me/botfather).
3. **Rust Toolchain:** Version 1.70 or higher.

## ⚙️ Prerequisites & Environment Setup

To successfully compile and run Kaspa Solo, your system must have the following external dependencies installed:

### 1. Mandatory Build Tools (C++ & CMake)
* **Windows:** `winget install cmake` (Ensure "Desktop development with C++" is selected in VS Build Tools).
* **Linux (Ubuntu/Debian):** `sudo apt update && sudo apt install cmake build-essential`

### 2. Multimedia Processing (FFmpeg)
* **Windows:** `winget install ffmpeg`
* **Linux:** `sudo apt install ffmpeg`

---

### 📝 Environment Configuration (`.env`)
Create a `.env` file in the project's root directory:
```env
BOT_TOKEN=your_telegram_bot_token_here
ADMIN_ID=your_telegram_user_id
WS_URL=ws://127.0.0.1:18110
RUST_LOG=info,kaspa_solo=debug
````

-----

### 📦 Step 0: Clone the Repository

Before proceeding with OS-specific instructions, clone the repository to your local machine or server:

```bash
git clone https://github.com/KaspaPulse/kaspa-solo.git
cd kaspa-solo
```

-----

### 🐧 Linux (Ubuntu/Debian) Deployment

**1. Install Dependencies & Rust:**

```bash
sudo apt update && sudo apt install -y curl build-essential pkg-config libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

**2. Compile the Engine:**

```bash
cargo build --release
```

**3. Create a Persistent `systemd` Background Service:**
This ensures the bot runs continuously and auto-restarts upon system reboots.

```bash
sudo nano /etc/systemd/system/kaspa-solo.service
```

Paste the following configuration (replace `your_username` with your actual Linux user):

```ini
[Unit]
Description=Kaspa Solo Enterprise Bot
After=network.target

[Service]
User=your_username
WorkingDirectory=/home/your_username/kaspa-solo
ExecStart=/home/your_username/kaspa-solo/target/release/kaspa-solo
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

Save (`Ctrl+O`, `Enter`, `Ctrl+X`) and initialize the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable kaspa-solo
sudo systemctl start kaspa-solo
```

*To monitor live enterprise logs:*

```bash
sudo journalctl -u kaspa-solo -f -o cat
```

-----

### 🪟 Windows Server Deployment

**1. Install Prerequisites:**

  * Install [Visual Studio C++ Build Tools](https://www.google.com/search?q=https://visualstudio.microsoft.com/visual-cpp-build-tools/).
  * Install Rust via [rustup.rs](https://rustup.rs/).

**2. Compile the Engine:**
Open `PowerShell` in the project directory:

```powershell
cargo build --release
```

**3. Create a Persistent Background Service via `NSSM`:**
NSSM (Non-Sucking Service Manager) is the industry standard for wrapping Windows executables into background services.

1.  Download and extract [NSSM](http://nssm.cc/download).
2.  Open `Command Prompt` as **Administrator**, navigate to the `win64` NSSM directory, and run:
    ```cmd
    nssm install KaspaSoloBot
    ```
3.  In the GUI that appears:
      * **Path:** Browse and select `C:\path\to\kaspa-solo\target\release\kaspa-solo.exe`
      * **Directory:** Browse and select the project root `C:\path\to\kaspa-solo`
4.  Click **Install service**.
5.  Start the service:
    ```cmd
    nssm start KaspaSoloBot
    ```

*(The bot is now running silently in the background and will survive system reboots).*

-----

## 📱 Command Reference

### 📌 Public Commands

| Command | Description |
| :--- | :--- |
| `/start` | Initializes the bot and renders the interactive UI keyboard. |
| `/add <address>` | Subscribe to real-time reward tracking for a wallet. |
| `/remove <address>` | Unsubscribe a wallet from tracking. |
| `/balance` | Query live balances and total UTXO count directly from the node. |
| `/blocks` | Audit total unspent mined blocks and their cumulative KAS value. |
| `/miner` | Calculate estimated local mining hashrate based on recent block frequency. |
| `/network` | Retrieve node health, global hashrate, DAA score, and supply metrics. |

### 👑 Admin Diagnostics (Secured Scope)

These commands are cryptographically restricted to the `ADMIN_ID` specified in the `.env` file.

| Command | Description |
| :--- | :--- |
| `/stats` | View enterprise analytics: Active users, tracked wallets, and Node Ping latency. |
| `/sys` | Hardware diagnostics: Monitor RAM utilization and thread health. |
| `/logs` | Fetch the last 25 lines of the deep telemetry `bot.log` file directly in Telegram. |
| `/broadcast` | Push a global announcement to all subscribed users. |
| `/pause` / `/resume` | Temporarily suspend or resume the background UTXO monitoring engine. |
| `/restart` | Gracefully terminate and reboot the Rust process. |

-----

## 🤝 Contributing

We embrace the open-source ethos. To contribute:

1.  **Fork** the repository.
2.  Create a **Feature Branch** (`git checkout -b feature/NextGenFeature`).
3.  Commit your changes (`git commit -m 'feat: Add NextGenFeature'`).
4.  Push to the branch (`git push origin feature/NextGenFeature`).
5.  Open a **Pull Request**.

-----

## 💖 Support the Project

Engineering a zero-warning, highly concurrent Rust engine requires countless hours of development and live-node testing. If this software has secured your operations or streamlined your node management, consider supporting the architect:

**Kaspa (KAS) Donation Address:**
`kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g`

-----

*Built with absolute mathematical precision for the Kaspa ecosystem.*
