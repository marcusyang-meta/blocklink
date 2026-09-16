# Recovery validation — 2026-09-15

Reconstructed from visible session source on baseline `5c4a130`; this is a new implementation commit, not recovery of the lost Git objects.

- Desktop production build: passed (`pnpm build`).
- Chinese/English key coverage and interpolation: 2 tests passed.
- Worker runtime: 5 tests passed, including managed host authentication, durable delivery, idempotency and revocation.
- Core/model/host/service tests: 70 passed, 1 failed, 8 ignored. The failure is `peer::tests::encrypted_sync_game_forwarding_and_live_revocation` with `联机邀请无效`. All five new managed/provision tests passed. This is not a clean backend test run.
- Full workspace test attempted with `--locked`: native desktop compilation blocked by missing system `glib-2.0` development package in this Linux environment.
- UI smoke passed using simulated native IPC: managed server page renders, EULA gates launch, submission carries a unique request ID. Screenshot: `screenshots/managed-hosts-preview.png`. Reproduce with `scripts/managed-host-ui.cjs`, Playwright available under `CODEX_PRIMARY_RUNTIME_NODE_MODULES`, and `BLOCKLINK_CHROMIUM_PATH` pointing to Chromium.
- Installer shell syntax and `git diff --check`: passed.

No real SSH host, end-to-end deployment, production Worker publication, or release asset publication has been tested/performed. The screenshot uses simulated host data. The headless release asset and Worker changes must be published before the new deployment flow can work against production. Git push was attempted but failed because GitHub HTTPS credentials were unavailable.
