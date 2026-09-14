# Modpacks

Search Modrinth in the Modpacks tab, choose a version and install, or import a local pack. Each installation creates an independent game and prepares its specified Minecraft, Fabric/Quilt/Forge/NeoForge and managed Java. Optional client files can be included or excluded.

## Contents and installation

Imports include the manifest, shared overrides and client-overrides, with client overrides taking precedence. Configuration, resource packs and shaders are retained. Server-only files and server-overrides are skipped. Mods enter shared content storage and are linked or cloned into games. Launching a pack does not automatically replace versions locked by its author; manual mod changes remain available.

Downloads verify SHA-512, SHA-1 and declared size, reuse verified caches, and support allowed HTTPS fallback URLs. Installation stages in a temporary instance on the same volume and publishes it only after the pack contents are complete. Path traversal, symbolic links, unsafe Windows names, duplicate files and case collisions are rejected. Limits are 512 MB per archive, 8 GB of manifest downloads and 4 GB of extracted overrides. Allowed sources include Modrinth CDN and the format's recommended GitHub/GitLab sources; redirects remain subject to HTTPS and host restrictions.

Tasks show actual transferred bytes and speed, with cancellation and retry. Cancellation takes effect between download chunks or processing stages; a pending network request may need to time out. Verified cached content is retained. If runtime preparation is interrupted after the complete pack is published, continue or repair the game to finish setup. Task history currently lasts only for the current backend session.

Launch failures offer retry, game-file repair, runtime settings, environment snapshot recovery and logs. Diagnostics use local log rules and cannot cover every crash. Recovery requires choosing an existing snapshot; arbitrary snapshots are not labeled known-good.

## Supported formats

- Modrinth `.mrpack` version 1.
- Complete MCBBS ZIP exports from HMCL/PCL: manifestVersion 1, mcbbs.packmeta or manifest.json with addons, optionally inside a single outer directory. Minecraft and supported loader components are matched automatically. Local manifest files are verified with SHA-1; mods, configuration, resource packs and shaders in overrides are retained.

Custom launch libraries or arguments, unknown components, missing remote files and CurseForge manifests are rejected before import. Arbitrary zipped launcher directories, HMCL dynamic server-update packs, CurseForge download manifests, automatic pack upgrades and treating any client pack as a server pack are unsupported. World deployment remains a separate feature.

CurseForge online downloads are not integrated because Blocklink has no configured API access. See the [provider's authentication announcement](https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/).

## Categories and filters

Categories come from Modrinth. Combine game and loader filters with downloads, relevance, recently updated, newest or follows sorting. Those filters continue into version selection. Stable releases are shown by default; prereleases can be enabled explicitly. Installed online packs are marked and can be opened directly. Local .mrpack imports use the same installation confirmation screen.

References: [Modrinth pack format](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack) and [project search](https://docs.modrinth.com/api/operations/searchprojects/).

## Deletion

Deleting a game or server permanently removes its worlds, configuration and local backups after typed name confirmation. Running or locked worlds block deletion. Rooms and bindings are removed; other games and the shared cache remain. Interrupted removal resumes from an internal, non-recoverable deleting directory on the next startup. Legacy recycle-bin data is not automatically emptied. See [deletion details](DELETION.md).

## Recorded validation

Tests cover atomic imports, shared mods across two instances, override precedence, path traversal, case collisions, client content selection, cancellation and one-time retry.

An isolated Modrinth install of Fabulously Optimized 8.2.0 on Minecraft 1.21.4/Fabric 0.19.3 was cancelled and resumed successfully. All 46 mods, configuration and resource packs were present, and launch checks passed. Reimporting the same .mrpack reused all 46 mods; game logs confirmed initialization and normal exit.

A complete MCBBS ZIP with the same 46 real mods imported successfully, retaining Chinese configuration and installing its specified game and Fabric. Missing files, invalid paths, case collisions, bad hashes and unavailable sources did not publish incomplete instances. That test round passed 39 service tests; seven external integration tests were not rerun.

[Chinese version](MODPACKS.zh-CN.md)
