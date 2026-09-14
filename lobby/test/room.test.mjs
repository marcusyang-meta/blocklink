import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {Miniflare} from 'miniflare';

test('real Worker runtime: private room, signaling, credentials gating and close', async () => {
  const mf = new Miniflare({modules: true, script: await readFile(new URL('../src/worker.js', import.meta.url), 'utf8'),
    compatibilityDate: '2026-07-01', durableObjects: {ROOMS: {className: 'Room', useSQLite: true}},
    bindings: {LOBBY_HOST_KEY: 'isolated-test-host-key', LOCAL_TEST: 'true', MAX_ROOMS: '2', MAX_MEMBERS: '2'}});
  const sockets = [];
  try {
    const call = (path, token, payload) => mf.dispatchFetch('http://lobby' + path, {method: payload === undefined ? 'GET' : 'POST',
      headers: token ? {Authorization: 'Bearer ' + token} : {}, body: payload === undefined ? undefined : JSON.stringify(payload)});
    assert.equal((await call('/api/rooms', '', {})).status, 401);
    const created = await call('/api/rooms', 'isolated-test-host-key', {name: '测试房间', minecraft: '1.21.1'});
    assert.equal(created.status, 201); const room = await created.json(), path = '/api/rooms/' + room.id;
    assert.equal((await call(path + '/join', '', {invitation: room.invitation})).status, 409);
    assert.equal((await call(path, '')).status, 401);
    const connect = async token => {
      const response = await mf.dispatchFetch('http://lobby' + path + '/socket', {headers: {Upgrade: 'websocket', Authorization: 'Bearer ' + token}});
      assert.equal(response.status, 101); const socket = response.webSocket; socket.accept(); sockets.push(socket); return socket;
    };
    const host = await connect(room.owner);
    assert.equal((await call(path + '/join', '', {invitation: 'wrong'})).status, 403);
    const guest = await (await call(path + '/join', '', {invitation: room.invitation, name: '朋友'})).json();
    assert.equal((await call(path + '/turn', guest.token, {})).status, 409);
    const friend = await connect(guest.token);
    const signal = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(Error('signal timeout')), 3000);
      host.addEventListener('message', event => { const data = JSON.parse(event.data); if (data.type === 'signal') {clearTimeout(timeout); resolve(data);} });
    });
    friend.send(JSON.stringify({type: 'signal', kind: 'offer', to: 'someone-else', payload: {sdp: 'test'}}));
    assert.deepEqual(await signal, {type: 'signal', kind: 'offer', from: guest.id, payload: {sdp: 'test'}});
    const status = await (await call(path, room.owner)).json();
    assert.equal(status.online, true); assert.equal(status.members.length, 1);
    assert.equal(JSON.stringify(status).includes(room.owner), false);
    assert.deepEqual(await (await call(path + '/turn', guest.token, {})).json(), {iceServers: [], localTest: true});
    assert.equal((await call(path + '/close', guest.token, {})).status, 403);
    assert.equal((await call(path + '/close', room.owner, {})).status, 200);
    assert.equal((await call(path + '/join', '', {invitation: room.invitation})).status, 410);
    assert.equal((await call('/init', '', {})).status, 404);
    assert.equal((await call('/registry', '', {})).status, 404);
    // Closed rooms release their registry slot; concurrent creation cannot exceed the cap.
    const attempts = await Promise.all(Array.from({length:3},()=>call('/api/rooms','isolated-test-host-key',{name:'quota'})));
    assert.deepEqual(attempts.map(r=>r.status).sort(),[201,201,429]);
  } finally {for (const ws of sockets) try {ws.close();} catch {} await mf.dispose();}
});
