# Blocklink

**More time for adventure.** A lightweight Minecraft Java Edition launcher built with Tauri, Rust and React. No Electron or Node runtime is required to use it.

[简体中文](README.md) · [Website](https://blocklink-lobby.junjie-f33.workers.dev/en/) · [Downloads](https://blocklink-lobby.junjie-f33.workers.dev/en/#download) · [Report a problem](https://github.com/marcusyang-meta/blocklink/issues/new/choose)

![Blocklink game library](lobby/public/assets/launcher.png)

## Play your way

- Separate games and modpacks, with automatic Minecraft, Java and loader installation.
- Browse Modrinth mods and modpacks by category and compatible game version. Import .mrpack or complete MCBBS ZIP exports from HMCL / PCL.
- Reuse identical mod files across games while keeping worlds and settings separate.
- Manage mods, shaders, compatibility plans and recovery snapshots.
- Import existing games and worlds, back up saves, or deploy a world to a local server.
- Host on your own computer and invite friends using Blocklink. Synchronize published server mods before joining.
- Switch between English and Simplified Chinese in Launcher settings.

## Download and start

Get the [Windows portable preview](https://blocklink-lobby.junjie-f33.workers.dev/en/#download), extract the ZIP and open Blocklink.exe. It needs Windows 10/11 x64 and WebView2. Choose a player, add a game or modpack, then press Play. Java and game files are downloaded as needed. Keep independent backups of important worlds.

The current local-player mode does not authenticate to online-mode servers. Microsoft account integration is implemented but Minecraft API access has not been approved. Do not assume Microsoft sign-in is working in this preview.

## Platform status

Windows builds are locally tested. Linux x64, Apple Silicon and Intel Mac previews passed native builds and service startup tests. Minecraft graphics, audio, input and shader verification on Mac/Linux is still pending. See [platform details](PLATFORMS.md) and the actual Release attachments.

## Development

Install Rust stable, Node 24, pnpm 11 and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
cd desktop
pnpm install --frozen-lockfile
pnpm build
node --test test/i18n.test.mjs
pnpm tauri build
```

From the repository root, run `cargo test --workspace --locked`. The [desktop workflow](.github/workflows/desktop.yml) builds native artifacts and portable archives on four runners. The [lobby workflow](.github/workflows/lobby.yml) tests the separate Cloudflare service. Neither publishes a release or deploys your server automatically.

## More information

[Getting started](docs/GETTING-STARTED.md) · [Changes](CHANGELOG.md) · [Modpacks](MODPACKS.md) · [Worlds](WORLDS.md) · [Multiplayer](NETWORK.md) · [Cloudflare deployment](lobby/README.md) · [Translations](desktop/I18N.md) · [Validation](VALIDATION.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Reports and suggestions are welcome in [GitHub Issues](https://github.com/marcusyang-meta/blocklink/issues). Never include credentials or private invitation links.

MIT licensed. Third-party notices are in [desktop/src-tauri/notices](desktop/src-tauri/notices). No Minecraft, Java or mod binaries are included in the source repository. Blocklink is an independent project and is not affiliated with, sponsored by or endorsed by Mojang or Microsoft.
