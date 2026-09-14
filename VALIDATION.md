# Validation

## Cross-network acceptance (2026-09-14): failed

- A real isolated Fabric 1.21.1 server loaded on Windows loopback. The host used the test-only `RTCIceTransportPolicy::Relay` setting with the production Cloudflare lobby. A GitHub-hosted Linux client ran the published 0.1.3 AppImage.
- The [cloud client](https://github.com/marcusyang-meta/blocklink/actions/runs/34826467602) rendered its launcher and reached room connection, but failed ICE gathering after 25 seconds (`Waiting for network candidates timed out`). Minecraft did not launch and no player joined the server. An earlier attempt failed before app startup because the harness selected two AppImages; that harness error was corrected.
- Separate [credential-free network diagnostics](https://github.com/marcusyang-meta/blocklink/actions/runs/34826840100) received STUN binding responses on UDP 3478/53, but TURN UDP 3478 timed out after three attempts. TURN UDP 53 and TLS 443/5349 were reachable. The local host received TURN UDP 3478 binding responses. These probes establish endpoint reachability, not authenticated TURN allocation or a selected ICE route; they ran on a separate GitHub runner.
- The current `webrtc` 0.20.5 transport skips secure and non-UDP TURN URLs. Blocklink waits for all gathering to complete before sending its offer. A stalled endpoint can therefore block negotiation, and TCP/TLS fallback is not implemented. This is an unresolved multiplayer reliability issue, not a passed cloud multiplayer check.
- The isolated host was stopped after diagnosis and its original authentication/port settings restored. Real cross-network world entry, room-close disconnect, automatic relay fallback and reconnect remain unverified.

## 0.1.3 cloud and native verification

- All four native builds, Rust tests and service checks passed: [Windows, Linux, Apple Silicon and Intel Mac](https://github.com/marcusyang-meta/blocklink/actions/runs/34818816053).
- The rendered launcher frontend successfully communicated with its service on all four cloud platforms. These are actual native app launches, not only browser mockups.
- Linux also automatically installed Java 21, Minecraft 1.21.1 and Fabric, launched the game and reached texture atlas initialization under Xvfb with Mesa software rendering.
- Supplemental Apple Silicon and Windows game runs installed Java/Minecraft/Fabric but did not complete graphics initialization. Thread diagnostics reached GLFW window creation and its native error dialog; the Windows runner reported Microsoft Hyper-V Video. These remain failed rendering checks, not proof of working gameplay. [Mac diagnostic](https://github.com/marcusyang-meta/blocklink/actions/runs/34821476521), [Windows diagnostic](https://github.com/marcusyang-meta/blocklink/actions/runs/34821505192).
- 65 local Rust tests passed. The browser-to-native settings-conflict flow preserved local files when dismissed, then backed them up and applied server settings when chosen. Native 0.1.2-to-0.1.3 update and deliberate early-crash rollback passed on Windows.
- All four downloaded updater payloads passed independent signature verification; modified payloads were rejected.
- The cloud harness now tolerates locked temporary WebView files during cleanup, avoiding an unrelated cleanup error after a successful Windows UI check.

## Update recovery development checks

- 64 Rust tests passed; seven external integrations remain opt-in. TypeScript and the production frontend build passed.
- An interactive Windows native smoke test replaced the launcher, safely handed over an idle 0.1.2 service, and received health confirmation from the rendered UI before removing the old executable.
- A second native run deliberately terminated the new launcher. The helper restored the exact previous executable and restarted it.
- Reproduce with `scripts/windows-update-smoke.py --new PATH --previous PATH --work-dir PATH` on an interactive Windows desktop. The script uses isolated data and retains fixtures for inspection.
- Sandboxed WebView startup did not acknowledge readiness and correctly triggered rollback; successful UI verification used normal desktop permissions.
- Legacy services without safe shutdown still require exiting the old launcher manually. They are rejected instead of silently used. macOS/Linux post-update recovery and interactive game checks remain unverified.

## 0.1.2 feature verification

- 61 Rust workspace tests passed; external integrations remain opt-in. A separate local Cloudflare-compatible lobby check transferred mods and configuration updates over real WebRTC, forwarded TCP and verified revocation.
- Native Windows EXE smoke checks covered server publication, client binding, repeated configuration sync, modpack export and graceful updater shutdown.
- Tests cover export/reimport, exclusion of private launcher data, preserved destination files, managed-file deletion, local conflicts and interrupted configuration recovery.
- Authentic signed update payloads verify against the bundled key; altered payloads are rejected.
- English and Chinese dictionary checks and the production frontend build passed. Browser verification caught and corrected narrow-window navigation overlap.
- Public update discovery skips incomplete releases and uses a static fallback during upstream outages.


## 2026-09-13 local Windows preview

- Windows MSVC release builds successfully. The packaged EXE starts its isolated native service, answers authenticated status requests and rejects unauthenticated calls.
- Rust workspace tests: 56 passed, 7 external integration tests ignored by default. Final Java architecture checks: 3 passed, including a real managed Java 21 process and rejection of a mismatched custom Java architecture.
- Frontend TypeScript and production build pass. Translation checks verify dictionary coverage, interpolation and system-language selection.
- Browser UI against an isolated real Rust service: Chinese/English switching, persistence after reload, modpack and installed-mod categories, game details and desktop settings layout checked. User-entered game names remain unchanged.
- Portable Windows archive verified with executable and third-party license notices. Published EXE SHA-256 matches the local release build.
- Previous Windows integration checks cover game and server installation, supported loader flows, modpack imports, world migration and local multiplayer. These do not guarantee every modpack, loader version or shader will work.

## Native release checks

Windows x64, Linux x64, Apple Silicon and Intel Mac passed the native GitHub Actions build, Rust and translation tests, desktop service startup and authenticated RPC checks. The corresponding ZIP, DMG, AppImage and DEB release assets have verified SHA-256 checksums. [Build evidence](https://github.com/marcusyang-meta/blocklink/actions/runs/34802210184).

## Not yet verified

- Physical GPU behavior, audio, input, native file dialogs and credential storage still need platform-specific verification on Mac/Linux. Linux cloud graphics initialization is covered above; cloud Mac gameplay did not pass.
- Microsoft device authorization is implemented, but Minecraft API access has not been approved. A 403 at the Minecraft endpoint is not proof of every possible underlying cause; do not claim successful authenticated game login.
- No CurseForge online integration. Unsupported package formats are rejected rather than presented as installed.

Tests do not replace preserving important worlds. Use isolated test data when validating deletion, migration and recovery.

[Chinese version](VALIDATION.zh-CN.md)
