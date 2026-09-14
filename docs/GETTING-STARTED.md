# Getting started

## Download and open

Choose your OS and processor on [GitHub Releases](https://github.com/marcusyang-meta/blocklink/releases).

- **Windows 10/11 x64:** extract the full ZIP and open Blocklink.exe. System WebView2 is required.
- **macOS, Apple Silicon or Intel:** extract the matching app ZIP or use the DMG. Builds are unsigned and not notarized.
- **Linux x64:** make the extracted AppImage executable and open it, or install the DEB. A compatible desktop environment and a Secret Service credential store are required.

Native builds and service startup tests passed on all four targets. Actual Minecraft graphics, audio, input and shader verification on Mac/Linux is still pending.

## Start a game

1. Choose English or Simplified Chinese in Launcher settings and select a player profile.
2. Add a game or choose a modpack. Java, Minecraft and the loader are prepared automatically.
3. Press Play. If some content cannot be adapted automatically, review the proposed compatibility plan.

Microsoft sign-in is not approved for Minecraft API access yet. Local-player profiles cannot join servers requiring authenticated accounts.

## Bring your worlds

Close the source game. In Worlds & saves, import a world folder, saves folder or .minecraft directory. Match the original game version, loader and mods first. Import creates a separate copy and preserves the original.

## Play together

Servers run on the host's computer. Create a server or deploy a world, read and accept the Minecraft EULA, then start it. Open a room and send its invitation to friends using Blocklink. They paste it to synchronize content and join. Keep the host computer and background service online. Hosting requires a configured Cloudflare lobby; see [deployment instructions](../lobby/README.md).

## Updates and backups

Replace the app files to update; game data stays in the OS user-data directory. A background host may continue after closing the window. If the executable is locked, save the new version separately rather than force-stopping a server that may be saving. Permanently deleting a game also deletes its worlds and local backups. Keep independent backups.

## Feedback

[Report an issue](https://github.com/marcusyang-meta/blocklink/issues/new/choose) with your app version, OS and reproduction steps. Do not include account tokens or private invitations.

[Chinese version](GETTING-STARTED.zh-CN.md)
