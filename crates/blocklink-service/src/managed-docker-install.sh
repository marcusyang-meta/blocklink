#!/bin/sh
# Runs as root after SSH host verification. Never mounts the Docker socket.
set -eu
stage=$1
data=/var/lib/blocklink-docker
name=blocklink-agent
[ "$(id -u)" = 0 ]
command -v docker >/dev/null
docker info >/dev/null
[ "$(uname -m)" = x86_64 ]
if [ -e /var/lib/blocklink/agent.json ]; then
  echo 'An existing native agent must be migrated separately; refusing concurrent management' >&2; exit 1
fi
if [ -L "$data" ]; then echo 'Data directory must not be a symlink' >&2; exit 1; fi
if [ -e "$data" ] && [ ! -f "$data/.managed-docker-install" ]; then
  echo 'Refusing to use an unmanaged data directory' >&2; exit 1
fi
existing=false
if docker container inspect "$name" >/dev/null 2>&1; then
  [ "$(docker inspect -f '{{ index .Config.Labels "dev.blocklink.managed" }}' "$name")" = true ] || { echo 'Container name is already in use' >&2; exit 1; }
  [ "$(docker inspect -f '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Source}}{{end}}{{end}}' "$name")" = "$data" ] || { echo 'Unexpected existing data mount' >&2; exit 1; }
  [ -f "$data/.managed-docker-install" ] || exit 1
  # Agent identity is immutable on repair. Do not silently take over another host.
  cmp -s "$data/agent.json" "$stage/agent.json" || { echo 'Existing host identity differs; restore its original registration' >&2; exit 1; }
  existing=true
fi
image="blocklink-managed:$(sha256sum "$stage/blocklink-service" | cut -c1-16)"
# Finish building and verify linkage before stopping an existing service.
# Keep agent credentials out of the Docker build context.
printf '%s\n' '*' '!managed.Dockerfile' '!blocklink-service' > "$stage/.dockerignore"
if ! docker build --pull -t "$image" -f "$stage/managed.Dockerfile" "$stage" > "$stage/build.log" 2>&1; then
  tail -20 "$stage/build.log" >&2; exit 1
fi
docker run --rm --network none --read-only --cap-drop ALL "$image" --version >/dev/null
if [ "$existing" = true ]; then
  [ "$(docker inspect -f '{{.State.Running}}' "$name")" = true ] || { echo 'Existing agent is stopped; inspect it before replacing' >&2; exit 1; }
  docker exec "$name" blocklink-service --root /data --request prepare-app-update >/dev/null
  docker stop -t 90 "$name" >/dev/null
  previous="blocklink-agent-previous-$(date +%s)"
  docker rename "$name" "$previous"
  # Retain the old container for manual recovery, but never start two agents on reboot.
  docker update --restart=no "$previous" >/dev/null
fi
install -d -m 700 -o 10001 -g 10001 "$data"
install -m 600 -o 10001 -g 10001 "$stage/agent.json" "$data/agent.json"
touch "$data/.managed-docker-install"
# Linux host networking supports dynamic game ports. RPC still binds loopback.
# :Z gives this dedicated directory a private SELinux label on Fedora.
if ! docker run -d --name "$name" --label dev.blocklink.managed=true \
    --restart unless-stopped --init --network host --read-only \
    --cap-drop ALL --security-opt no-new-privileges:true \
    --stop-timeout 90 --tmpfs /tmp:rw,nosuid,nodev,size=512m \
    -v "$data:/data:Z" \
    "$image" >/dev/null; then
  echo 'Container launch failed; data retained. Inspect Docker before retrying.' >&2; exit 1
fi
docker inspect -f '{{.State.Running}}' "$name" | grep -qx true
