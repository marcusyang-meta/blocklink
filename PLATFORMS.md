# Blocklink desktop builds / 桌面版本

The UI supports English and Simplified Chinese. It follows the system language on first use. Change it in Launcher settings → Language; the preference is saved locally. Switching does not rename games, mods or worlds. Publisher descriptions, game logs and unrecognized backend details retain their original text.

启动器支持简体中文和英文，首次跟随系统语言。启动器设置 → 语言中可立即切换，选择保存在本机，不修改游戏、模组或存档名称。作者介绍、游戏原始日志和未识别的后台详情保留原文。

| Platform | Portable format | Verification |
| --- | --- | --- |
| Windows 10 / 11, x64 | Blocklink.exe in ZIP | Built locally; needs WebView2 |
| macOS 11+, Apple Silicon | Blocklink.app in ZIP; DMG | Native build and service tests passed; game verification pending |
| macOS 11+, Intel | Blocklink.app in ZIP; DMG | Native build and service tests passed; game verification pending |
| Linux x64, Ubuntu 22.04+ baseline | AppImage; DEB | Native build and service tests passed; game verification pending |

Windows x64, Linux x64, Apple Silicon and Intel Mac all passed native builds and service tests. Preview packages are available on GitHub Releases. A service test does not replace verifying Minecraft windows, sound, input, shaders, file pickers and credential storage on each OS. Preview builds are unsigned. macOS signing and notarization are not configured.

Windows x64、Linux x64、Apple Silicon 和 Intel Mac 均已通过原生构建和后台启动测试，并提供预览包。自动测试之外，还需验证各系统的游戏窗口、声音、输入、光影、文件选择和凭据库。测试版未签名，macOS 签名与公证尚未配置。

## Platform behavior

- Java downloads match the OS and required architecture. Modern Apple Silicon games use ARM Java and libraries. Older releases without ARM libraries use Intel Java and require Rosetta 2 already installed. Blocklink does not install Rosetta or accept its terms automatically.
- Windows uses WebView2; macOS uses system WebKit; Linux uses WebKitGTK. AppImage needs a compatible desktop and executable permission. APPIMAGE_EXTRACT_AND_RUN=1 avoids requiring FUSE with supported AppImage runtimes.
- Linux hosts launch through a separate AppImage process so closing the UI does not remove the background host's mounted files.
- Microsoft credentials use the OS vault; Linux requires a working Secret Service, such as GNOME Keyring or KWallet with Secret Service enabled. No plaintext-token fallback is used.
- Game data stays in the OS application-data directory. A portable app does not store worlds next to the executable.

## Build

Put this directory at the repository root (Cargo.toml, desktop/, scripts/, .github/). The Build Windows, macOS and Linux workflow creates four native builds, tests Rust and translations, starts the actual background host, verifies RPC authentication and packages licenses and SHA-256 checksums. Artifacts are uploaded to the workflow run; public releases are not published automatically.

For local builds, install platform prerequisites, Rust stable, Node 24 and pnpm 11. In desktop/, run pnpm install --frozen-lockfile, node --test test/i18n.test.mjs, then pnpm tauri build. macOS packaging needs macOS; Linux packaging needs Linux.

References: https://v2.tauri.app/distribute/ and https://v2.tauri.app/distribute/appimage/
