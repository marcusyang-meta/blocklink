# Validation

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

- Actual Minecraft windows, graphics, audio, input, file dialogs and credential storage still need platform-specific verification on Mac/Linux.
- Microsoft device authorization is implemented, but Minecraft API access has not been approved. A 403 at the Minecraft endpoint is not proof of every possible underlying cause; do not claim successful authenticated game login.
- No CurseForge online integration. Unsupported package formats are rejected rather than presented as installed.

Tests do not replace preserving important worlds. Use isolated test data when validating deletion, migration and recovery.

[Chinese version](VALIDATION.zh-CN.md)
