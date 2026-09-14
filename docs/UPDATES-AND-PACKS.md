# App updates, modpack upgrades, and shared configuration

Blocklink checks for signed app updates when opened. Use **Check for app updates** in the sidebar to check again, then **Download and update**. Downloads are verified using the public key bundled with the launcher. Games, hosted servers, and downloads must finish before the background service shuts down for replacement.

Windows updates replace the portable executable without installing an MSI or NSIS package. macOS updates replace the `.app`; Linux updates use the AppImage format. A Linux `.deb` installation should be updated using its package manager or switched to the portable AppImage. The old preview has no updater and needs one manual download to adopt this feature. OS code signing and macOS notarization are separate from update signatures.

## Local recovery changes awaiting release

The launcher checks both service version and protocol before connecting. An idle service supporting safe shutdown is replaced automatically; a busy or unsupported old service displays a startup error in the launcher instead of closing the window or accepting incompatible commands.

On Windows, the update helper retains the previous executable until the frontend has rendered a successful service response and the new process has remained alive for another five seconds. Early exit or a 60-second startup timeout restores and restarts the previous executable. If restoration fails, the backup remains available and its path is recorded in `update-error.txt`. This startup check does not cover crashes after the confirmation window or a power failure interrupting the helper. macOS/Linux still use the upstream installer and do not yet have this additional recovery path.

## Modpacks

Open an installed pack's project in **Modpacks**, select a release, and choose **Upgrade to selected version**. This creates a separate instance and copies worlds. The original instance is retained, including personal modifications, as the rollback option. The new instance uses the pack author's configuration and scripts; local mod or configuration changes are not merged into it. World migration currently requires an identical Minecraft version and refuses a running source instance. A failed migration blocks launching the incomplete copy until the job succeeds.

In a game's installation tab, **Export modpack** writes a `.mrpack` containing mods, configuration, default configurations, scripts, resource packs and shaders. Existing destination files are never overwritten. Worlds, accounts, logs and server lists are excluded. Configuration and scripts may contain private information; review them and redistribution permissions before sharing the archive.

## Server configuration

In a hosted server's settings, enable **Sync configuration and scripts**. This explicitly publishes `config`, `defaultconfigs`, `kubejs`, and `scripts`; remove credentials and private information before enabling it. The next publish or server start snapshots these directories into a verified bundle. Changes while the server is running are not silently republished.

Cloudflare lobby invitations and local server bindings synchronize the bundle before launch. Only previously managed files are removed; unrelated local files remain. Conflicting local edits stop synchronization with the affected filename. Move the conflicting file aside to keep a copy, then retry. Directory replacements have a recovery journal, and an interrupted replacement restores the previous directories on the next service start. Old direct HTTPS/iroh invitations refuse these environments and ask for a lobby invitation instead.

## Publishing updates

GitHub Actions signs platform payloads using the encrypted `TAURI_SIGNING_PRIVATE_KEY` repository secret. Never commit the private key. The build workflow also produces ordinary downloadable packages when signing credentials are unavailable, but these cannot be advertised as automatic updates.

After all four native builds pass, run the existing manually triggered release workflow to attach packages and publish `latest.json`. The website automatically discovers complete release feeds from the public repository and caches them for five minutes. Incomplete releases are skipped. Increase the application version for each update; metadata records the version used for each build. No manual website deployment is needed to advertise subsequent complete releases. Releases are not published automatically on push.

`lobby/public/updates/latest.json` is the fallback feed during GitHub outages or rate limiting. Refresh it periodically from a verified complete release. Existing release assets are preserved; the publication workflow reads the actual published signature when a locally built Windows payload was uploaded before CI completed.

Validation includes modpack export/reimport, private-file exclusions, existing-file preservation, configuration update/removal/conflict handling, interrupted directory recovery, and a local Cloudflare-compatible lobby test that transfers both mods and configuration over real WebRTC connections.
