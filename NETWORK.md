# Multiplayer through the Cloudflare lobby

New invitations create Cloudflare rooms. Hosts and friends discover each other through a public Worker, exchange WebRTC connection information, and connect directly or through Cloudflare TURN. See [lobby deployment](lobby/README.md), included as LOBBY.md in applicable portable distributions.

The lobby and game server are separate: Cloudflare runs discovery and connection services, while the host's computer runs Minecraft. An HTTPS invitation carries short-lived room access credentials, with a default lifetime of 12 hours. Closing the room revokes the invitation and disconnects participants. New rooms do not use Iroh's public relays.

## Legacy Iroh invitation compatibility

The UI no longer creates legacy Iroh invitations, but existing invitations remain importable. The original flow published a server's mod environment and created an invitation; a friend imported it, created an instance and selected synchronization and joining. Its original automatic setup supported Java, Minecraft and Fabric.

The Rust backend embeds Iroh 1.2 and encrypted QUIC streams. It establishes a reachable path, attempts NAT traversal and switches to direct connectivity when possible, otherwise using an Iroh Relay. No system virtual adapter, separate VPN or manual port forwarding is required. Automatic router mapping through portmapper is not enabled.

Minecraft connects to a random loopback TCP port on the player's computer. Blocklink forwards it to the hosted game's port over the encrypted connection. The same protocol transfers the published mod manifest and missing SHA-512 objects, without exposing the HTTPS synchronization port publicly. It permits only the selected server's game port and published client mods, not arbitrary host/port forwarding.

Legacy invitations use the public Iroh relay by default, without a Blocklink-operated relay or bandwidth guarantee. Relays see connection metadata such as endpoints and IP addresses, while game and file content remains end-to-end encrypted. Content is not uploaded to persistent cloud storage. Long-running deployments need their own assessment of network quality and relay availability.

### Invitations and revocation

Invitations pin the host's public key and carry a separate server-access token. Share them only with intended players. Revocation rejects future synchronization and connections and terminates existing connections using the old token in approximately one second. It cannot recall already downloaded files. Regenerated invitations must be imported again; significant address or relay changes can also require regeneration.

Host identity persists in the local data directory. Only hosts that previously generated an invitation restore a network endpoint automatically. Ordinary unpaired instances do not initialize multiplayer networking for offline launch. Exiting the game cleans up its local forwarding listener.

### Self-hosted legacy relays

A compatible relay needs a public address, domain and valid TLS certificate. Stop the legacy networking endpoint before changing its relay HTTPS URL, then regenerate invitations. Relays requiring dedicated client authentication tokens are not integrated. Blocklink does not automatically buy or deploy a cloud server. See [Iroh relay documentation](https://docs.iroh.computer/add-a-relay) for the deployed version.

### Recorded checks and limits

- Two isolated Windows backends passed encrypted mod transfer, bidirectional TCP forwarding, malformed-invitation rejection, rejection of unpublished files and revocation of active connections.
- A separate test disabled all direct IP transports, completed the same checks through the public relay, and asserted that the actual path was relay.
- Local tests do not establish traversal success across every carrier or NAT type. Mac/Linux game-level testing remains pending.
- Forwarding covers Minecraft Java TCP only; additional UDP services such as voice mods are unsupported.
- Tunneling does not bypass server authentication or solve account and Realms access. The legacy acceptance server required authenticated accounts; current explicitly configured local/offline server behavior is documented in [world deployment](WORLDS.md).
- The legacy synchronization scope covers published mods, not configuration, scripts or resource packs. Game/Fabric version mismatches block synchronization. Bound clients do not join with an old environment after connection or synchronization failure.
- The host must stay online and awake. Interrupted Minecraft sessions require rejoining; restored transport reachability is not seamless game-session recovery.

Protocol reference: [Iroh Rust 1.2](https://docs.rs/iroh/1.2.0/iroh/).

[Chinese version](NETWORK.zh-CN.md)
