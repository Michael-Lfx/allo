#!/usr/bin/env bash
set -euo pipefail

# Flowy Voice Companion macOS Host Installation Script (Wave 4 T4-02)

echo "Installing Flowy Voice Companion macOS LaunchDaemon..."

mkdir -p "/Library/Application Support/Flowy/voice-companion/models"
mkdir -p "/etc/voice-companion"

if [ -f "./target/release/voice-companion" ]; then
    install -m 755 ./target/release/voice-companion /usr/local/bin/voice-companion
elif [ -f "./voice-companion" ]; then
    install -m 755 ./voice-companion /usr/local/bin/voice-companion
elif [ -f "./target/release/ai-mate" ]; then
    install -m 755 ./target/release/ai-mate /usr/local/bin/voice-companion
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cp "${SCRIPT_DIR}/com.flowy.voicecompanion.plist" /Library/LaunchDaemons/com.flowy.voicecompanion.plist
chown root:wheel /Library/LaunchDaemons/com.flowy.voicecompanion.plist
chmod 644 /Library/LaunchDaemons/com.flowy.voicecompanion.plist

launchctl load -w /Library/LaunchDaemons/com.flowy.voicecompanion.plist
echo "Flowy Voice Companion macOS service loaded successfully."
