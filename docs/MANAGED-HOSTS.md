# Managed Linux hosts — recovered development preview

Reconstructed from the visible development-session code on baseline `5c4a130`. The lost commits `60e5aa3` and `e8bd7dd` are not present; this is a new recovery commit, not a byte-for-byte restoration of those Git objects. Do not reuse earlier validation claims as validation of this recovery.

## Flow

Configure the existing lobby address and host registration credential in launcher settings. In Remote servers, register a host, enter SSH address/username/password or private key, verify its fingerprint, and select a release containing the headless executable and checksum. The initial target is x86_64 Ubuntu, Debian, or Fedora with systemd, root or passwordless sudo, reachable SSH and outbound HTTPS. The bootstrap installs runtime prerequisites, checks the executable before replacing an installation, creates an unprivileged `blocklink` user and a systemd service. Existing active jobs or games block repair.

The app manages server creation/install/start/stop, log retrieval, Minecraft console commands, memory/port settings, Modrinth mods, world backups, independent restoration and world switching. Text settings editing is restricted to `server.properties`, operator/whitelist/ban JSON files and `config/`; 16 KB limit, no path traversal/symlinks, stopped server required for writes, optimistic SHA-256 conflict checking and original-file backups.

## Trust and protocol

SSH credentials are used locally for bootstrap, never saved in jobs or sent to the lobby. Private keys use a temporary private file for libssh2 portability and are removed after authentication. Separate owner/agent tokens are retained in the local OS credential store; agent configuration is installed mode 0600 in a mode 0700 remote data directory. The existing privileged lobby registration key gates enrollment. This is not yet an account/billing product.

`ManagedHost` Durable Objects are separate from multiplayer rooms: guest invites do not grant administration. The owner submits commands; the agent polls outbound HTTPS. Local RPC remains loopback-only. Cloud snapshots exclude launcher settings and Microsoft account state. Configuration contents and log snippets requested through management do pass through the cloud.

| Route | Credential | Purpose |
| --- | --- | --- |
| `POST /api/hosts` | Lobby registration key | Enroll a client-generated host identity |
| `GET /api/hosts/:id` | Owner | State and recent command results |
| `POST /api/hosts/:id/commands` | Owner | Submit command with request UUID |
| `POST /api/hosts/:id/poll` | Agent | Heartbeat, completion and next command |
| `POST /api/hosts/:id/revoke` | Owner | Disable management; running games continue |

Commands are durable, ordered, and redelivered with the same UUID. The agent writes a dispatch journal before executing; uncertain crashes return an interrupted outcome instead of replaying mutations. Dispatch acceptance can return a job ID; actual installation success is determined by that background job. Jobs persist locally and unfinished jobs become explicit errors after restart. The open app retains IDs for uncertain submission retries; frontend submissions are not persisted across a full app restart, so inspect history before submitting again.

Each host permits 20 pending commands and 500 retained records. Undelivered queued commands expire after 24 hours, finished records retain a seven-day deduplication window, and agent journal IDs are currently retained indefinitely. Snapshot/result size limits are explicit. The text editor and command queue are not a bulk file transfer channel.

## Release and remaining work

Run `.github/workflows/headless.yml` on the release commit, attach its `blocklink-service-linux-x86_64` and `.sha256` files to that GitHub release, deploy the Worker `HOSTS` binding and `v2-managed-hosts` migration, and distribute the updated desktop app. The headless workflow builds artifacts but does not publish them. CI builds on Ubuntu 22.04; remote hosts must have compatible glibc/runtime libraries. Old releases without these assets cannot provision hosts.

Real SSH/systemd/Minecraft acceptance, native platform packages and keychain round-trip still require validation. No production deployment is implied by local tests. Firewalls, router NAT and game-port exposure are not automatically changed. Remote lobby invitation creation, bulk world/file uploads, scheduled/off-host backups, automatic game-instance restart after host reboot and agent self-update/rollback remain follow-up work. Systemd keeps the host service alive; game instances themselves do not auto-resume after reboot. SIGTERM requests normal game save/stop subject to the systemd timeout. The prior binary is retained, but automatic binary rollback is not implemented.

Recovery validation is recorded in `RECOVERY-VALIDATION.md`. Any screenshot uses a simulated native IPC fixture and is not proof of real remote deployment.
