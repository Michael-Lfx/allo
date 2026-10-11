#!/usr/bin/env bash
set -euo pipefail

# Flowy AI Mate macOS Host Installation Script (Wave 4 T4-02)

echo "Installing Flowy AI Mate macOS LaunchDaemon..."

mkdir -p "/Library/Application Support/Flowy/ai-mate/models"
mkdir -p "/etc/ai-mate"

if [ -f "./target/release/ai-mate" ]; then
    install -m 755 ./target/release/ai-mate /usr/local/bin/ai-mate
elif [ -f "./ai-mate" ]; then
    install -m 755 ./ai-mate /usr/local/bin/ai-mate
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cp "${SCRIPT_DIR}/com.flowy.aimate.plist" /Library/LaunchDaemons/com.flowy.aimate.plist
chown root:wheel /Library/LaunchDaemons/com.flowy.aimate.plist
chmod 644 /Library/LaunchDaemons/com.flowy.aimate.plist

launchctl load -w /Library/LaunchDaemons/com.flowy.aimate.plist
echo "Flowy AI Mate macOS service loaded successfully."
