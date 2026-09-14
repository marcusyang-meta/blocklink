# Validation / 验收状态

## 2026-09-13 local Windows preview

- Windows MSVC release builds successfully. The packaged EXE starts its isolated native service, answers authenticated status requests and rejects unauthenticated calls.
- Rust workspace tests: 56 passed, 7 external integration tests ignored by default. Final Java architecture checks: 3 passed, including a real managed Java 21 process and rejection of a mismatched custom Java architecture.
- Frontend TypeScript and production build pass. Translation checks verify dictionary coverage, interpolation and system-language selection.
- Browser UI against an isolated real Rust service: Chinese/English switching, persistence after reload, modpack and installed-mod categories, game details and desktop settings layout checked. User-entered game names remain unchanged.
- Portable Windows archive verified with executable and third-party license notices. Published EXE SHA-256 matches the local release build.
- Previous Windows integration checks cover game and server installation, supported loader flows, modpack imports, world migration and local multiplayer. These do not guarantee every modpack, loader version or shader will work.

## Native release checks

Linux x64 and Apple Silicon Mac passed the native GitHub Actions build, Rust and translation tests, desktop service startup and authenticated RPC checks. The corresponding ZIP, DMG, AppImage and DEB release assets have verified SHA-256 checksums. [Build evidence](https://github.com/marcusyang-meta/blocklink/actions/runs/34802210184).

## Not yet verified / 尚未验收

- Intel Mac build completion is pending. Actual Minecraft windows, graphics, audio, input, file dialogs and credential storage still need platform-specific verification on Mac/Linux.
- Microsoft device authorization is implemented, but Minecraft API access has not been approved. A 403 at the Minecraft endpoint is not proof of every possible underlying cause; do not claim successful authenticated game login.
- No CurseForge online integration. Unsupported package formats are rejected rather than presented as installed.

Tests do not replace preserving important worlds. Use isolated test data when validating deletion, migration and recovery.
