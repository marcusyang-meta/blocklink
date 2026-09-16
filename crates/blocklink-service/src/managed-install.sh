#!/bin/sh
set -eu
stage=$1
[ "$(id -u)" = 0 ]
if [ -e /var/lib/blocklink-docker/agent.json ]; then
  echo "An existing Docker agent must be migrated separately" >&2; exit 1
fi
[ -f /etc/os-release ]
. /etc/os-release
case "$ID" in
  ubuntu|debian) export DEBIAN_FRONTEND=noninteractive; apt-get update -qq; apt-get install -y -qq ca-certificates openssl libdbus-1-3 >/dev/null ;;
  fedora) dnf install -y -q ca-certificates openssl dbus-libs >/dev/null ;;
  *) echo 'Supported systems: Ubuntu, Debian, Fedora' >&2; exit 1 ;;
esac
"$stage/blocklink-service" --version >/dev/null
if [ -e /usr/local/bin/blocklink-service ] && [ ! -f /var/lib/blocklink/.managed-install ]; then
  echo 'An unmanaged Blocklink installation already exists; refusing to replace it' >&2; exit 1
fi
if systemctl is-active --quiet blocklink.service; then
  runuser -u blocklink -- /usr/local/bin/blocklink-service --root /var/lib/blocklink --request prepare-app-update >/dev/null
  systemctl stop blocklink.service
fi
if ! id blocklink >/dev/null 2>&1; then useradd --system --home-dir /var/lib/blocklink --shell /usr/sbin/nologin blocklink; fi
install -d -m 700 -o blocklink -g blocklink /var/lib/blocklink
install -m 755 "$stage/blocklink-service" /usr/local/bin/blocklink-service.new
if [ -f /usr/local/bin/blocklink-service ]; then cp -p /usr/local/bin/blocklink-service /usr/local/bin/blocklink-service.previous; fi
mv /usr/local/bin/blocklink-service.new /usr/local/bin/blocklink-service
install -m 600 -o blocklink -g blocklink "$stage/agent.json" /var/lib/blocklink/agent.json
cat > /etc/systemd/system/blocklink.service <<'UNIT'
[Unit]
Description=Blocklink managed Minecraft host
Wants=network-online.target
After=network-online.target
[Service]
Type=simple
User=blocklink
Group=blocklink
WorkingDirectory=/var/lib/blocklink
ExecStart=/usr/local/bin/blocklink-service --root /var/lib/blocklink
Restart=always
RestartSec=5
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/blocklink
TimeoutStopSec=90
[Install]
WantedBy=multi-user.target
UNIT
touch /var/lib/blocklink/.managed-install
systemctl daemon-reload
systemctl enable --now blocklink.service >/dev/null
systemctl is-active --quiet blocklink.service
