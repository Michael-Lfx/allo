#!/usr/bin/env bash
set -euo pipefail

# Flowy AI Mate Linux Host Installation Script (Wave 4 T4-01)
# Ensures non-privileged user isolation, directory structure, systemd unit, and watchdog.

echo "Installing Flowy AI Mate Voice Companion Daemon..."

# 1. Create dedicated system user and group
if ! id -u aimate >/dev/null 2>&1; then
    echo "Creating system user 'aimate'..."
    useradd -r -s /usr/sbin/nologin -d /var/lib/ai-mate -m -c "Flowy AI Mate Service" aimate
fi

# 2. Prepare directories with strict permission isolation
mkdir -p /var/lib/ai-mate/models
mkdir -p /etc/ai-mate
chown -R aimate:aimate /var/lib/ai-mate
chmod 750 /var/lib/ai-mate

# 3. Copy binary
if [ -f "./target/release/ai-mate" ]; then
    install -m 755 ./target/release/ai-mate /usr/local/bin/ai-mate
elif [ -f "./ai-mate" ]; then
    install -m 755 ./ai-mate /usr/local/bin/ai-mate
fi

# 4. Install systemd unit
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
install -m 644 "${SCRIPT_DIR}/ai-mate.service" /etc/systemd/system/ai-mate.service

# 5. Reload and enable service
systemctl daemon-reload
systemctl enable ai-mate.service

echo "Flowy AI Mate installed successfully. Start with: systemctl start ai-mate"
