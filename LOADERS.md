# Minecraft versions and mod loaders

Games and hosted servers can use Vanilla, Fabric, NeoForge, Forge or Quilt.

## Version selection and Java

- Versions come from Mojang's manifest. Blocklink lists stable Minecraft releases from 1.13 onward, including the 26.x naming scheme, without a fixed upper version limit. Snapshots, prereleases and 1.12.2 or earlier are outside this support range.
- The required Java major version comes from official game metadata. Blocklink downloads the matching platform's Temurin runtime, falling back from JRE to JDK availability. A custom Java path can be supplied in instance settings. Validation supports Java 8 and retains module-image checks for Java 9 and later.
- A listed version can enter the installation flow; it does not mean every game/platform combination has been tested. Mac/Linux game testing remains pending. Older releases may lack native ARM libraries. See [platform behavior](PLATFORMS.md).
- Existing games and loaders are not upgraded automatically. Back up worlds before upgrades. Renaming a mod cannot convert it between loader families.

## Fabric and NeoForge

Fabric uses its official metadata service. NeoForge uses the official Maven version list starting with Minecraft 1.20.2. The latest matching stable release is selected by default; listed prereleases can be chosen explicitly. Legacy NeoForge coordinates for 1.20.1 are unsupported, and Forge is never substituted for NeoForge.

NeoForge installers are downloaded from official Maven, verified with SHA-1, and run only against Blocklink-managed directories. Client patches are installed into the internal runtime; server installers produce their own platform argument files. Details are written to installer.log. Servers launch through win_args.txt or unix_args.txt rather than treating the installer output as an ordinary server.jar.

## Forge and Quilt

Forge versions are queried for the exact game version and sorted numerically. Installer coordinates include that game version, official checksums are verified, and inherited game metadata must match. Modern servers use platform argument files; older generated Forge launch JARs have a separate launch path.

Quilt uses official Meta client profiles and the official server installer. Installer downloads use Maven's accompanying SHA-256. The previously checked 0.15.1 Meta hash differed from the Maven file; the actual artifact matched Maven's checksum, so verification uses the same repository rather than disabling checks.

Quilt's game query can return historical loaders. For Minecraft 26.x, Blocklink's minimum is 0.30.1; the latest stable version is selected by default, with newer prereleases available explicitly. This is Blocklink's support baseline, not a statement about every historical upstream combination.

## Mods, synchronization and joining

Modrinth search, version selection and dependencies are filtered by loader. Local imports read Fabric/Quilt JSON or Forge/NeoForge TOML; Quilt imports also recognize Fabric metadata. Modrinth downloads for Quilt require explicit Quilt compatibility. Deployment, published environments and pre-launch synchronization enforce exact game and loader versions. Forge and NeoForge remain distinct environments; both friends need a launcher version that recognizes the chosen loader.

Bound local servers are joined automatically on launch: Minecraft 1.13–1.19 uses legacy connection arguments, and 1.20+ uses Quick Play. Offline-profile multiplayer in 1.16.5 was limited during acceptance and automatic joining did not pass; older versions must not all be described as verified for offline multiplayer.

Historical Windows game-join checks for 26.2 included Forge 65.1.3 with GeckoLib 5.5.5, Quilt 0.30.1 with Lithium 0.25.3, and NeoForge 26.2.0.87. Worlds and mods from different loaders are not freely interchangeable. See [validation](VALIDATION.md).

[Chinese version](LOADERS.zh-CN.md)
