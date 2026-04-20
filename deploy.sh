#!/bin/bash
# ==============================================================================
# Kaspa Pulse - Enterprise Deployment Script
# ==============================================================================

echo "🚀 [1/4] Stopping Kaspa Pulse Service..."
sudo systemctl stop kaspa-pulse.service

echo "⬇️ [2/4] Pulling Latest Code from Git..."
git pull origin main

echo "⚙️ [3/4] Compiling Enterprise Engine (Forcing ALL Features)..."
# The --all-features flag guarantees AI capabilities are never left behind
cargo build --release --all-features

echo "▶️ [4/4] Restarting Service..."
sudo systemctl start kaspa-pulse.service

echo "✅ Deployment Successful! Showing live logs:"
sudo journalctl -u kaspa-pulse.service -f -n 20