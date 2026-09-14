# World migration and server deployment

Open Worlds & saves in a game instance.

## Import from another launcher

1. Prepare an instance with exactly the original Minecraft version, loader, mods and configuration.
2. Save the world and close the original game or server.
3. Choose Import from another launcher and select a world directory containing level.dat, a saves directory, or a game directory. Scanning reaches four levels; select saves directly for deeper layouts.
4. Select the world, confirm its environment, and copy it into Blocklink. The original remains intact; repeated imports create independent copies.

Only Java Edition folders are supported. Extract ZIP files first. Import does not convert Bedrock worlds, migrate launcher accounts or automatically resolve modpack dependencies. Unknown or mismatched game versions are rejected to prevent accidental upgrades or downgrades.

## Deploy a world as a server

Choose Deploy as server beside a world and enter a name and port. Blocklink installs a server with the source instance's game version, loader and memory settings. It reuses required server mods, excludes mods marked client-only, and copies config, defaultconfigs, kubejs, scripts and the entire world.

Optional player migration copies embedded single-player inventory and position into the current profile's server player file. When the original UUID can be identified, statistics and advancements are copied too. Entity ownership references, such as pets, are not rewritten. Worlds without embedded Player data retain their original player files.

Offline mode requires an explicit choice, listens only on 127.0.0.1, and can be reached through Blocklink invitations. Offline names do not establish an authenticated identity. Inventory migration requires matching profile and server authentication modes.

Select immediate startup and accept the Minecraft EULA to deploy and start in one operation. Otherwise, deployment opens the server page. The host computer runs and saves the world; Cloudflare provides discovery and connectivity, not Minecraft hosting.

## Existing worlds and recovery

Importing into a stopped server preserves its existing world and switches to a new copy. You can switch back; subsequent progress is not merged. This is preservation before replacement, not scheduled backup.

Copying checks source hashes before and after transfer, rejects symlinks and special files, and attempts to acquire session.lock. External launcher locking differs across platforms, so close the source game first. World files are never hard-linked.

The active-world setting changes only after copying completes. Failed deployments are marked incomplete and cannot start; the source world remains intact. Retrying from the source creates a new server. Interrupted copies do not become active worlds.

Fabric, NeoForge, Forge and Quilt deployment requires matching game and loader versions. See [loader support](LOADERS.md).


[Chinese version](WORLDS.zh-CN.md)
