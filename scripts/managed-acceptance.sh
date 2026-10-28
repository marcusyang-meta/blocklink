#!/usr/bin/env bash
# Full-chain acceptance for the Blocklink managed-host Docker deployment.
#
# Runs the whole flow on ONE machine (a GitHub ubuntu-22.04 runner):
#   build the service -> serve it as a fake release -> local Miniflare lobby
#   -> operator service drives a real Docker deployment over SSH to localhost
#   -> (phase instance) create/launch a real Minecraft server, SLP-ping it,
#      stop it, run a repair deployment and verify container retention.
#
# Usage: scripts/managed-acceptance.sh [deploy|instance|full]
# Env:   ACCEPTANCE_WORK (scratch dir; default mktemp), SKIP_BUILD=1 to
#        reuse an existing target/release/blocklink-service.
set -euo pipefail

PHASE="${1:-instance}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SVC="$ROOT/target/release/blocklink-service"
TAG="v0.1.4-acceptance"
REG_KEY="acceptance-registration-key"
GAME_PORT=25601
WORK="${ACCEPTANCE_WORK:-$(mktemp -d)}"
OP="$WORK/operator"
LOGS="$WORK/logs"
mkdir -p "$OP" "$LOGS" "$WORK/dl"

HOST_ID=""
FINGERPRINT=""
DUMPED=0

say() { echo "[acceptance] $*"; }
fail() { echo "[acceptance] FAILED: $*" >&2; dump; exit 1; }

dump() {
  [ "$DUMPED" = "1" ] && return 0
  DUMPED=1
  {
    echo "===== operator status ====="
    timeout 15 "$SVC" --root "$OP" --request status 2>&1 | head -c 4000 || true
    echo "===== deployment state ====="
    cat "$OP"/deployment-*.json 2>/dev/null || true
    echo "===== docker ps -a ====="
    docker ps -a 2>&1 || true
    echo "===== docker logs blocklink-agent (tail) ====="
    docker logs blocklink-agent 2>&1 | tail -100 || true
    echo "===== operator log (tail) ====="
    tail -100 "$LOGS/operator.log" 2>/dev/null || true
    echo "===== lobby log (tail) ====="
    tail -100 "$LOGS/lobby.log" 2>/dev/null || true
  } | tee "$LOGS/diagnostics.txt" >&2
}
trap dump ERR

rpc() { # rpc <action> <payload-json> [extra cli args...]
  local action="$1" payload="$2"
  shift 2
  printf '%s' "$payload" >"$WORK/payload.json"
  "$SVC" --root "$OP" --request "$action" --payload-file "$WORK/payload.json" "$@"
}

host_status() { rpc managed-status "{\"hostId\":\"$HOST_ID\"}"; }

status_jq() { host_status | jq -r "$1"; }

wait_until() { # wait_until <description> <timeout-seconds> <command...>
  local desc="$1" timeout_s="$2"
  shift 2
  local elapsed=0
  until "$@" >/dev/null 2>&1; do
    [ "$elapsed" -ge "$timeout_s" ] && fail "timeout waiting for: $desc"
    sleep 5
    elapsed=$((elapsed + 5))
  done
  say "ready: $desc"
}

wait_url() { wait_until "$1" "$2" curl -fsS "$3"; }

wait_command() { # wait_command <request-id> <timeout> -> prints command result JSON
  local id="$1" timeout_s="$2" elapsed=0 st cs
  while :; do
    st="$(host_status)"
    cs="$(jq -r --arg id "$id" '.commands[]? | select(.id==$id) | .status' <<<"$st" | head -1)"
    case "$cs" in
      done)
        jq -c --arg id "$id" '.commands[] | select(.id==$id) | .result' <<<"$st"
        return 0
        ;;
      error)
        jq -c --arg id "$id" '.commands[] | select(.id==$id)' <<<"$st" >&2
        fail "managed command $id failed"
        ;;
    esac
    [ "$elapsed" -ge "$timeout_s" ] && fail "timeout waiting for command $id (status: ${cs:-missing})"
    sleep 5
    elapsed=$((elapsed + 5))
  done
}

wait_agent_job() { # wait_agent_job <job-id> <timeout> -> prints job JSON from the agent snapshot
  local id="$1" timeout_s="$2" elapsed=0 st js
  while :; do
    st="$(host_status)"
    js="$(jq -r --arg id "$id" '.snapshot.jobs[]? | select(.id==$id) | .status' <<<"$st" | head -1)"
    case "$js" in
      done)
        jq -c --arg id "$id" '.snapshot.jobs[] | select(.id==$id)' <<<"$st"
        return 0
        ;;
      error)
        jq -c --arg id "$id" '.snapshot.jobs[] | select(.id==$id)' <<<"$st" >&2
        fail "agent job $id failed"
        ;;
    esac
    [ "$elapsed" -ge "$timeout_s" ] && fail "timeout waiting for agent job $id (status: ${js:-missing})"
    sleep 5
    elapsed=$((elapsed + 5))
  done
}

instance_not_running() {
  [ "$(status_jq ".snapshot.instances[]? | select(.port==$GAME_PORT) | .running" | head -1)" != "true" ]
}

submit_command() { # submit_command <action> <payload-json> -> prints request id
  local action="$1" payload="$2" rid
  rid="$(cat /proc/sys/kernel/random/uuid)"
  rpc managed-command "$(jq -cn --arg h "$HOST_ID" --arg r "$rid" --arg a "$action" --argjson p "$payload" \
    '{hostId:$h,requestId:$r,action:$a,payload:$p}')" >"$LOGS/command-$action.json"
  echo "$rid"
}

deploy() {
  local payload
  payload="$(jq -cn --arg h "$HOST_ID" --arg fp "$FINGERPRINT" --rawfile key "$WORK/id_ed25519" --arg tag "$TAG" \
    '{hostId:$h,host:"127.0.0.1",port:22,username:"root",privateKey:$key,fingerprint:$fp,releaseTag:$tag,deploymentMode:"docker"}')"
  printf '%s' "$payload" >"$WORK/deploy-payload.json"
  say "managed-deploy (docker) via SSH to localhost"
  timeout 1500 "$SVC" --root "$OP" --request managed-deploy --payload-file "$WORK/deploy-payload.json" --wait >"$LOGS/deploy.json"
  cat "$LOGS/deploy.json"
}

assert_deploy() {
  say "asserting Docker deployment state"
  docker ps --format '{{.Names}}' | grep -qx 'blocklink-agent' || fail "blocklink-agent container is not running"
  local img
  img="$(docker inspect -f '{{.Config.Image}}' blocklink-agent)"
  [ -n "$img" ] || fail "cannot resolve agent image"
  [ "$(docker inspect -f '{{.Config.User}}' blocklink-agent)" = "10001:10001" ] || fail "container user is not 10001:10001"
  [ "$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' blocklink-agent)" = "true" ] || fail "container rootfs is not read-only"
  docker inspect blocklink-agent | jq -e '.[0].HostConfig.CapDrop | index("ALL")' >/dev/null || fail "CapDrop does not contain ALL"
  docker inspect blocklink-agent | jq -e '[.[0].Mounts[] | select(.Source | test("docker\\.sock"))] | length == 0' >/dev/null || fail "docker socket is mounted into the agent container"
  docker inspect blocklink-agent | jq -e '[.[0].Mounts[] | select(.Source == "/var/lib/blocklink-docker" and .Destination == "/data")] | length == 1' >/dev/null || fail "persistent /data mount missing"
  [ "$(sudo stat -c %a /var/lib/blocklink-docker/agent.json)" = "600" ] || fail "agent.json is not mode 600"
  docker run --rm --entrypoint /bin/sh "$img" -c \
    'test ! -e /data/agent.json && test -z "$(find /usr/local /etc -name agent.json 2>/dev/null)"' >/dev/null ||
    fail "agent.json leaked into the built image"
  host_status | jq -e '.online == true' >/dev/null || fail "managed-status does not report the agent online"
  say "deployment assertions passed (image: $img)"
}

phase_instance() {
  say "creating Minecraft server instance via managed command"
  local rid result job_id job instance_id
  rid="$(submit_command create '{"name":"Acceptance","minecraft":"1.21.1","loader":"vanilla","memory":2048,"port":25601}')"
  result="$(wait_command "$rid" 180)"
  job_id="$(jq -r '.jobId' <<<"$result")"
  [ -n "$job_id" ] && [ "$job_id" != "null" ] || fail "create command returned no jobId: $result"
  job="$(wait_agent_job "$job_id" 1500)"
  instance_id="$(jq -r '.result.id' <<<"$job")"
  [ -n "$instance_id" ] && [ "$instance_id" != "null" ] || fail "create job returned no instance id: $job"
  say "instance created: $instance_id"

  say "launching instance (EULA accepted)"
  rid="$(submit_command launch "$(jq -cn --arg id "$instance_id" '{id:$id,eula:true}')")"
  result="$(wait_command "$rid" 180)"
  job_id="$(jq -r '.jobId' <<<"$result")"
  wait_agent_job "$job_id" 900 >/dev/null

  say "waiting for the Minecraft server to answer SLP on port $GAME_PORT"
  local elapsed=0
  until python3 "$ROOT/scripts/slp-ping.py" 127.0.0.1 "$GAME_PORT" >"$WORK/slp.json" 2>"$LOGS/slp.err"; do
    [ "$elapsed" -ge 600 ] && fail "Minecraft server did not answer SLP within 600s"
    sleep 10
    elapsed=$((elapsed + 10))
  done
  jq -e '.version.name and .players' "$WORK/slp.json" >/dev/null || fail "SLP response lacks version/players: $(cat "$WORK/slp.json")"
  say "SLP ok: $(jq -c '{version:.version.name,players:.players.online}' "$WORK/slp.json")"

  # The repair path (prepare-app-update) refuses to run while a game is up,
  # by design, so stop the instance before repairing.
  say "stopping instance before repair"
  rid="$(submit_command stop "$(jq -cn --arg id "$instance_id" '{id:$id}')")"
  wait_command "$rid" 300 >/dev/null
  wait_until "instance stopped" 180 instance_not_running

  say "running repair deployment"
  deploy

  say "asserting repair retention"
  local prev
  prev="$(docker ps -a --format '{{.Names}}' | grep -E '^blocklink-agent-previous-[0-9]+$' | head -1 || true)"
  [ -n "$prev" ] || fail "no retained blocklink-agent-previous-* container after repair"
  [ "$(docker inspect -f '{{.State.Running}}' "$prev")" = "false" ] || fail "previous container $prev is still running"
  [ "$(docker inspect -f '{{.HostConfig.RestartPolicy.Name}}' "$prev")" = "no" ] || fail "previous container $prev restart policy is not 'no'"
  assert_deploy
  host_status | jq -e --argjson port "$GAME_PORT" '[.snapshot.instances[] | select(.port==$port)] | length == 1' >/dev/null ||
    fail "instance is no longer listed in the agent snapshot after repair"
  say "instance assertions passed (instance retained after repair: $instance_id)"
}

preflight() {
  say "preflight (phase=$PHASE, work=$WORK)"
  docker info >/dev/null 2>&1 || fail "docker daemon is not reachable"
  for tool in cargo node pnpm jq python3 curl ssh-keygen; do
    command -v "$tool" >/dev/null || fail "missing tool: $tool"
  done
  [ -x /usr/sbin/sshd ] || command -v sshd >/dev/null || fail "openssh-server is not installed"
  command -v gnome-keyring-daemon >/dev/null || fail "gnome-keyring is not installed"
  command -v dbus-daemon >/dev/null || fail "dbus is not installed"
}

build_service() {
  if [ "${SKIP_BUILD:-0}" = "1" ] && [ -x "$SVC" ]; then
    say "reusing existing service binary"
    return
  fi
  say "building blocklink-service (release)"
  (cd "$ROOT" && cargo build --release -p blocklink-service --locked) >"$LOGS/cargo.log" 2>&1 ||
    { tail -50 "$LOGS/cargo.log" >&2; fail "cargo build failed"; }
  [ -x "$SVC" ] || fail "service binary missing after build"
}

setup_keyring() {
  say "starting session dbus + gnome-keyring (operator credential store)"
  export DBUS_SESSION_BUS_ADDRESS="$(dbus-daemon --session --fork --print-address)"
  [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ] || fail "could not start a session dbus"
  # --unlock and --start cannot be combined, and zero bytes of password
  # input leave the login keyring uncreated (Secret Service writes then
  # fail with "no result found"). Unlock with a throwaway password first,
  # then ensure the secrets component is running.
  printf 'acceptance' | gnome-keyring-daemon --unlock >"$LOGS/keyring.log" 2>&1 || true
  gnome-keyring-daemon --start --components=secrets >>"$LOGS/keyring.log" 2>&1 || true
  ls -la "$HOME/.local/share/keyrings/" >>"$LOGS/keyring.log" 2>&1 || true
  # Functional probe: prove the Secret Service actually stores secrets
  # before the operator depends on it for lobby + host credentials.
  if command -v secret-tool >/dev/null; then
    printf 'probe' | secret-tool store --label='blocklink acceptance probe' service 'Blocklink Acceptance' account probe 2>>"$LOGS/keyring.log" ||
      { cat "$LOGS/keyring.log" >&2; fail "keyring probe store failed"; }
    secret-tool lookup service 'Blocklink Acceptance' account probe >/dev/null 2>&1 ||
      { cat "$LOGS/keyring.log" >&2; fail "keyring probe lookup failed"; }
    say "keyring probe ok"
  fi
}

start_download_server() {
  say "serving the just-built binary as release $TAG on 127.0.0.1:8799"
  mkdir -p "$WORK/dl/$TAG"
  cp "$SVC" "$WORK/dl/$TAG/blocklink-service-linux-x86_64"
  (cd "$WORK/dl/$TAG" && sha256sum blocklink-service-linux-x86_64 >blocklink-service-linux-x86_64.sha256)
  (cd "$WORK/dl" && nohup python3 -m http.server 8799 --bind 127.0.0.1 >"$LOGS/download.log" 2>&1 &)
  export BLOCKLINK_SERVICE_DOWNLOAD_BASE="http://127.0.0.1:8799"
  wait_url "download server" 60 "http://127.0.0.1:8799/$TAG/blocklink-service-linux-x86_64.sha256"
}

start_lobby() {
  say "installing lobby dependencies and starting the Miniflare acceptance lobby"
  (cd "$ROOT/lobby" && pnpm install --frozen-lockfile) >"$LOGS/pnpm.log" 2>&1 ||
    { tail -30 "$LOGS/pnpm.log" >&2; fail "pnpm install failed"; }
  (cd "$ROOT/lobby" && LOBBY_HOST_KEY="$REG_KEY" nohup node acceptance-server.mjs >"$LOGS/lobby.log" 2>&1 &)
  wait_url "acceptance lobby" 120 "http://127.0.0.1:8787/health"
}

setup_sshd() {
  say "configuring sshd on localhost as the deployment target (root, key auth)"
  ssh-keygen -t ed25519 -N '' -f "$WORK/id_ed25519" >/dev/null
  sudo ssh-keygen -A >/dev/null 2>&1 || true
  sudo install -d -m 755 /run/sshd
  sudo install -d -m 700 /root/.ssh
  sudo install -m 600 "$WORK/id_ed25519.pub" /root/.ssh/authorized_keys
  printf 'PermitRootLogin prohibit-password\nPubkeyAuthentication yes\nPasswordAuthentication no\n' |
    sudo tee /etc/ssh/sshd_config.d/99-blocklink-acceptance.conf >/dev/null
  sudo service ssh restart >/dev/null 2>&1 || sudo service ssh start >/dev/null 2>&1 || sudo /usr/sbin/sshd
  wait_until "sshd root key login" 90 ssh -i "$WORK/id_ed25519" -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=5 root@127.0.0.1 true
}

start_operator() {
  say "starting the operator service (root: $OP)"
  nohup "$SVC" --root "$OP" >"$LOGS/operator.log" 2>&1 &
  wait_until "operator RPC" 90 bash -c '"$1" --root "$2" --request status >/dev/null 2>&1' _ "$SVC" "$OP"
}

configure_and_register() {
  say "configuring lobby + registering host"
  rpc lobby-configure "{\"url\":\"http://127.0.0.1:8787\",\"hostKey\":\"$REG_KEY\"}" >/dev/null
  local reg probe
  reg="$(rpc managed-register '{"name":"acceptance"}')"
  HOST_ID="$(jq -r '.id' <<<"$reg")"
  [ "${#HOST_ID}" = "32" ] || fail "managed-register returned no host id: $reg"
  probe="$(rpc managed-probe '{"host":"127.0.0.1","port":22}')"
  FINGERPRINT="$(jq -r '.fingerprint' <<<"$probe")"
  [ -n "$FINGERPRINT" ] && [ "$FINGERPRINT" != "null" ] || fail "managed-probe returned no fingerprint: $probe"
  say "host registered: $HOST_ID (fingerprint: $FINGERPRINT)"
}

main() {
  case "$PHASE" in deploy | instance | full) ;; *) fail "unknown phase: $PHASE" ;; esac
  preflight
  build_service
  setup_keyring
  start_download_server
  start_lobby
  setup_sshd
  start_operator
  configure_and_register
  deploy
  assert_deploy
  if [ "$PHASE" = "instance" ] || [ "$PHASE" = "full" ]; then
    phase_instance
  fi
  if [ "$PHASE" = "full" ]; then
    say "Crafty/Fedora phases not yet implemented"
  fi
  say "ACCEPTANCE PASSED (phase=$PHASE)"
}

main "$@"
