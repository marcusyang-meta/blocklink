// Local acceptance lobby: serves the real lobby worker under Miniflare so
// the managed-host deploy flow can run end to end without Cloudflare.
// Usage: LOBBY_HOST_KEY=<key> node acceptance-server.mjs  (after pnpm install)
import {readFile} from 'node:fs/promises';
import {Miniflare} from 'miniflare';

const mf = new Miniflare({
  modules: true,
  script: await readFile(new URL('./src/worker.js', import.meta.url), 'utf8'),
  compatibilityDate: '2026-07-01',
  durableObjects: {
    HOSTS: {className: 'ManagedHost', useSQLite: true},
    ROOMS: {className: 'Room', useSQLite: true},
  },
  bindings: {LOBBY_HOST_KEY: process.env.LOBBY_HOST_KEY || 'acceptance-registration-key'},
  host: '127.0.0.1',
  port: 8787,
});

const url = await mf.ready;
console.log(`acceptance lobby ready on ${url}`);

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, async () => {
    await mf.dispose();
    process.exit(0);
  });
}
