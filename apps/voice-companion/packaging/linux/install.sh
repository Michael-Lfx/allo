#!/usr/bin/env bash
set -euo pipefail

# Flowy Voice Companion Linux Host Installation Script (Wave 4 T4-01)
# Ensures non-privileged user isolation, directory structure, systemd unit, and watchdog.

echo "Installing Flowy Voice Companion Daemon..."

# 1. Create dedicated system user and group
if ! id -u voicecompanion >/dev/null 2>&1; then
    echo "Creating system user 'voicecompanion'..."
    useradd -r -s /usr/sbin/nologin -d /var/lib/voice-companion -m -c "Flowy Voice Companion Service" voicecompanion
fi

# 2. Prepare directories with strict permission isolation
mkdir -p /var/lib/voice-companion/models
mkdir -p /etc/voice-companion
chown -R voicecompanion:voicecompanion /var/lib/voice-companion
chmod 750 /var/lib/voice-companion

# 3. Copy binary
if [ -f "./target/release/voice-companion" ]; then
    install -m 755 ./target/release/voice-companion /usr/local/bin/voice-companion
elif [ -f "./voice-companion" ]; then
    install -m 755 ./voice-companion /usr/local/bin/voice-companion
elif [ -f "./target/release/ai-mate" ]; then
    install -m 755 ./target/release/ai-mate /usr/local/bin/voice-companion
fi

# 4. Install systemd unit
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
install -m 644 "${SCRIPT_DIR}/voice-companion.service" /etc/systemd/system/voice-companion.service

# 5. Reload and enable service
systemctl daemon-reload
systemctl enable voice-companion.service

echo "Flowy Voice Companion installed successfully. Start with: systemctl start voice-companion"
