const encoder = new TextEncoder();
const json = (value, status = 200) => Response.json(value, {status, headers: {'Cache-Control': 'no-store'}});
const secret = () => Array.from(crypto.getRandomValues(new Uint8Array(32)), b => b.toString(16).padStart(2, '0')).join('');
const digest = async value => Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', encoder.encode(value))), b => b.toString(16).padStart(2, '0')).join('');
const bearer = request => request.headers.get('Authorization')?.replace(/^Bearer /, '') || '';
const limit = (value, fallback, max) => Math.max(1, Math.min(Number(value) || fallback, max));
async function body(request) {
  const reader = request.body?.getReader(); if (!reader) throw new Error('请求为空');
  const chunks = []; let size = 0;
  for (;;) {const {done, value} = await reader.read(); if (done) break; size += value.length; if (size > 65536) {await reader.cancel(); throw new Error('请求过大');} chunks.push(value);}
  const bytes = new Uint8Array(size); let offset = 0; for (const chunk of chunks) {bytes.set(chunk, offset); offset += chunk.length;}
  return JSON.parse(new TextDecoder().decode(bytes));
}
function label(value, max = 80) { return typeof value === 'string' ? value.trim().slice(0, max) : ''; }
function metadata(value) {
  return {name: label(value.name) || 'Blocklink 房间', minecraft: label(value.minecraft, 40), loader: label(value.loader, 60),
    running: value.running === true, modCount: Math.min(10000, Math.max(0, Number(value.modCount) || 0))};
}

export default {
  async fetch(request, env) {
    try {
      const url = new URL(request.url);
      if (url.pathname==='/updates/latest.json' && request.method==='GET') return latestUpdate(request,env);
      if (url.pathname === '/health') return json({service: 'blocklink-lobby', version: 1, turnConfigured: !!(env.TURN_KEY_ID && env.TURN_KEY_API_TOKEN)});
      if ((request.method === 'GET' || request.method === 'HEAD') && (['/', '/privacy', '/en', '/en/', '/en/privacy', '/updates/latest.json'].includes(url.pathname) || url.pathname.startsWith('/assets/') || url.pathname.startsWith('/downloads/'))) {
        if (!env.ASSETS) return json({error: '页面暂不可用'}, 503);
        const assetUrl=new URL(request.url);
        const asset=await env.ASSETS.fetch(new Request(assetUrl,request));
        const response=new Response(asset.body,asset);
        response.headers.set('Referrer-Policy','no-referrer');
        response.headers.set('X-Content-Type-Options','nosniff');
        response.headers.set('Content-Security-Policy',"default-src 'none'; img-src 'self'; style-src 'self'; script-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'");
        if(url.pathname.endsWith('.exe')) response.headers.set('Content-Disposition','attachment; filename="Blocklink.exe"');
        if(url.pathname==='/updates/latest.json') response.headers.set('Cache-Control','public, max-age=300');
        return response;
      }
      if (request.method === 'GET' && /^\/invite\/[a-f0-9]{32}$/.test(url.pathname)) return invitePage();
      if (url.pathname === '/api/rooms' && request.method === 'POST') {
        if (!env.LOBBY_HOST_KEY || await digest(bearer(request)) !== await digest(env.LOBBY_HOST_KEY)) return json({error: '开房凭据无效'}, 401);
        const input = metadata(await body(request));
        return env.ROOMS.get(env.ROOMS.idFromName('registry')).fetch(new Request('https://room/registry', {method: 'POST', body: JSON.stringify(input)}));
      }
      const match = url.pathname.match(/^\/api\/rooms\/([a-f0-9]{32})(\/join|\/socket|\/turn|\/close)?$/);
      if (!match) return json({error: '地址不存在'}, 404);
      // Only these explicit routes reach a room; the registry cannot be called publicly.
      return env.ROOMS.get(env.ROOMS.idFromName(match[1])).fetch(request);
    } catch { return json({error: '请求无效'}, 400); }
  }
};

export async function latestUpdate(request,env,remote=fetch) {
  let reason='no-complete-release';
  const cache=typeof caches==='undefined'?null:caches.default;
  const key=new Request('https://blocklink.jyang.dev/updates/latest.json');
  const hit=await cache?.match(key);if(hit)return hit;
  try {
    if(env.LOCAL_TEST!=='true') {
      const listing=await remote('https://github.com/marcusyang-meta/blocklink/releases.atom',{headers:{'User-Agent':'Blocklink-updates','Accept':'application/atom+xml'},signal:AbortSignal.timeout(10000)});
      reason=`github-${listing.status}`;
      const atom=listing.ok?await listing.text():'';
      const tags=[...new Set([...atom.matchAll(/<link\b[^>]*href="https:\/\/github\.com\/marcusyang-meta\/blocklink\/releases\/tag\/(v[0-9A-Za-z._-]+)"/g)].map(m=>m[1]))].slice(0,10);
      for(const tag of tags) {
        const result=await remote(`https://github.com/marcusyang-meta/blocklink/releases/download/${tag}/latest.json`,{signal:AbortSignal.timeout(10000)});reason=`asset-${result.status}`;if(!result.ok)continue;
        const feed=await result.json();
        if(typeof feed.version!=='string'||!['windows-x86_64','darwin-aarch64','darwin-x86_64','linux-x86_64'].every(p=>typeof feed.platforms?.[p]?.signature==='string'&&feed.platforms[p].url?.startsWith('https://github.com/marcusyang-meta/blocklink/releases/download/')))continue;
        const response=Response.json(feed,{headers:{'Cache-Control':'public, max-age=300','X-Update-Source':'release'}});await cache?.put(key,response.clone());return response;
      }
    }
  }catch{reason='upstream-error';/* Keep the last deployed feed available during GitHub outages or rate limiting. */}
  const asset=await env.ASSETS.fetch(request);const response=new Response(asset.body,asset);response.headers.set('Cache-Control','public, max-age=300');response.headers.set('X-Update-Source',`fallback-${reason}`);return response;
}

export class Room {
  constructor(ctx, env) { this.ctx = ctx; this.env = env; }
  sockets() { return this.ctx.getWebSockets().filter(ws => ws.readyState === 1); }
  host() { return this.sockets().find(ws => ws.deserializeAttachment()?.role === 'host'); }
  async info() {
    const room = await this.ctx.storage.get('room');
    if (!room || room.expiresAt <= Date.now()) return null;
    return room;
  }
  async view() {
    const room = await this.info();
    if (!room) return {closed: true};
    const host = this.host()?.deserializeAttachment();
    return {...room.metadata, expiresAt: room.expiresAt, online: !!host && Date.now() - host.lastSeen < 75000,
      members: this.sockets().filter(ws => ws.deserializeAttachment().role === 'guest').map(ws => ({id: ws.deserializeAttachment().id, name: ws.deserializeAttachment().name}))};
  }
  async broadcast() { const data = JSON.stringify({type: 'room', room: await this.view()}); for (const ws of this.sockets()) { try { ws.send(data); } catch {} } }
  async identify(request, room) {
    const hash = await digest(bearer(request));
    if (hash === room.ownerHash) return {id: 'host', role: 'host', name: '房主'};
    const session = await this.ctx.storage.get('session:' + hash);
    return session && session.expiresAt > Date.now() ? {...session, hash} : null;
  }
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === '/registry') {
      // A single registry bounds concurrent rooms even under parallel create requests.
      return this.ctx.blockConcurrencyWhile(async () => {
        const now = Date.now();
        const rooms = (await this.ctx.storage.get('rooms') || []).filter(r => r.expiresAt > now);
        if (request.method === 'DELETE') {const {id} = await body(request); await this.ctx.storage.put('rooms', rooms.filter(r => r.id !== id)); return json({ok: true});}
        if (rooms.length >= limit(this.env.MAX_ROOMS, 20, 1000)) return json({error: '大厅房间数已达上限，请等待旧房间结束'}, 429);
        const id = secret().slice(0, 32), owner = secret(), invitation = secret();
        const expiresAt = now + limit(this.env.ROOM_TTL_SECONDS, 43200, 86400) * 1000;
        const room = {id, metadata: metadata(await body(request)), ownerHash: await digest(owner), inviteHash: await digest(invitation), expiresAt};
        const result = await this.env.ROOMS.get(this.env.ROOMS.idFromName(id)).fetch(new Request('https://room/init', {method: 'POST', body: JSON.stringify(room)}));
        if (!result.ok) return json({error: '创建房间失败'}, 500);
        rooms.push({id, expiresAt}); await this.ctx.storage.put('rooms', rooms);
        await this.ctx.storage.setAlarm(Math.min(...rooms.map(r => r.expiresAt)));
        return json({id, owner, invitation, expiresAt}, 201);
      });
    }
    if (url.pathname === '/init') {
      if (await this.ctx.storage.get('room')) return json({error: '房间已存在'}, 409);
      const room = await body(request);
      await this.ctx.storage.put('room', room); await this.ctx.storage.setAlarm(room.expiresAt);
      return json({ok: true});
    }
    const room = await this.info();
    if (!room) return json({error: '房间已关闭或邀请已过期'}, 410);
    if (url.pathname.endsWith('/join') && request.method === 'POST') {
      const input = await body(request);
      if (typeof input.invitation !== 'string' || await digest(input.invitation) !== room.inviteHash) return json({error: '邀请无效'}, 403);
      if (!this.host()) return json({error: '房主已离线'}, 409);
      const sessions = await this.ctx.storage.list({prefix: 'session:'});
      for (const [key, value] of sessions) if (value.expiresAt < Date.now()) { await this.ctx.storage.delete(key); sessions.delete(key); }
      if (sessions.size >= limit(this.env.MAX_MEMBERS, 8, 32) * 2) return json({error: '房间已满，请稍后重试'}, 429);
      const token = secret(), id = secret().slice(0, 16);
      await this.ctx.storage.put('session:' + await digest(token), {id, role: 'guest', name: label(input.name, 32) || '朋友', expiresAt: Math.min(room.expiresAt, Date.now() + 15 * 60000)});
      return json({token, id, room: await this.view()});
    }
    const identity = await this.identify(request, room);
    if (!identity) return json({error: '房间凭据无效或已过期'}, 401);
    if (url.pathname.endsWith('/close') && request.method === 'POST') {
      if (identity.role !== 'host') return json({error: '只有房主可以关闭房间'}, 403);
      for (const ws of this.sockets()) { ws.send(JSON.stringify({type: 'closed'})); ws.close(1000, 'room closed'); }
      const credentials = await this.ctx.storage.list({prefix: 'turn:'});
      if (this.env.TURN_KEY_ID && this.env.TURN_KEY_API_TOKEN) {
        const usernames = new Set([...credentials.values()].flatMap(c => c.value.iceServers).map(s => s.username).filter(Boolean));
        this.ctx.waitUntil(Promise.allSettled([...usernames].map(username => fetch(`https://rtc.live.cloudflare.com/v1/turn/keys/${encodeURIComponent(this.env.TURN_KEY_ID)}/credentials/${encodeURIComponent(username)}/revoke`, {
          method: 'POST', headers: {Authorization: 'Bearer ' + this.env.TURN_KEY_API_TOKEN}, signal: AbortSignal.timeout(10000)
        }))));
      }
      await this.ctx.storage.deleteAll();
      await this.env.ROOMS.get(this.env.ROOMS.idFromName('registry')).fetch(new Request('https://room/registry', {method: 'DELETE', body: JSON.stringify({id: room.id})}));
      return json({ok: true});
    }
    if (url.pathname.endsWith('/turn') && request.method === 'POST') {
      if (!this.sockets().some(ws => ws.deserializeAttachment().id === identity.id) || !this.host()) return json({error: '请先连接房间'}, 409);
      if (!this.env.TURN_KEY_ID || !this.env.TURN_KEY_API_TOKEN) {
        if (this.env.LOCAL_TEST === 'true') return json({iceServers: [], localTest: true});
        return json({error: '大厅尚未配置 Cloudflare TURN'}, 503);
      }
      const cacheKey = 'turn:' + identity.id, cached = await this.ctx.storage.get(cacheKey);
      if (cached && cached.refreshAt > Date.now()) return json(cached.value);
      const reserved = await this.ctx.storage.transaction(async txn => {
        const count = await txn.get('issuedCredentials') || 0;
        if (count >= 64) return false;
        await txn.put('issuedCredentials', count + 1); return true;
      });
      if (!reserved) return json({error: '此房间的连接凭据已达上限，请由房主重新开房'}, 429);
      const ttl = Math.max(60, Math.ceil((room.expiresAt - Date.now()) / 1000));
      const response = await fetch(`https://rtc.live.cloudflare.com/v1/turn/keys/${encodeURIComponent(this.env.TURN_KEY_ID)}/credentials/generate-ice-servers`, {
        method: 'POST', headers: {'Authorization': 'Bearer ' + this.env.TURN_KEY_API_TOKEN, 'Content-Type': 'application/json'}, body: JSON.stringify({ttl}), signal: AbortSignal.timeout(10000)
      });
      if (!response.ok) return json({error: 'Cloudflare TURN 凭据签发失败'}, 502);
      const {iceServers} = await response.json(); const value = {iceServers};
      if (!Array.isArray(value.iceServers)) return json({error: 'TURN 返回格式无效'}, 502);
      await this.ctx.storage.put(cacheKey, {value, refreshAt: Date.now() + 5 * 60000});
      return json(value);
    }
    if (url.pathname.endsWith('/socket') && request.headers.get('Upgrade')?.toLowerCase() === 'websocket') {
      if (identity.role === 'guest' && !this.host()) return json({error: '房主已离线'}, 409);
      const old = this.sockets().find(ws => ws.deserializeAttachment().id === identity.id);
      if (!old && identity.role === 'guest' && this.sockets().length > limit(this.env.MAX_MEMBERS, 8, 32)) return json({error: '房间已满'}, 429);
      if (old) old.close(1000, 'reconnected');
      const pair = new WebSocketPair(), client = pair[0], server = pair[1];
      server.serializeAttachment({...identity, lastSeen: Date.now(), window: Date.now(), count: 0});
      this.ctx.acceptWebSocket(server);
      server.send(JSON.stringify({type: 'welcome', id: identity.id, room: await this.view()}));
      await this.broadcast(); await this.ctx.storage.setAlarm(Math.min(Date.now() + 60000, room.expiresAt));
      return new Response(null, {status: 101, webSocket: client});
    }
    if (request.method === 'GET') return json(await this.view());
    return json({error: '不支持的操作'}, 405);
  }
  async webSocketMessage(ws, message) {
    if (typeof message !== 'string' || encoder.encode(message).length > 65536) { ws.close(1009, 'message too large'); return; }
    const member = ws.deserializeAttachment();
    if (Date.now() - member.window > 10000) { member.window = Date.now(); member.count = 0; }
    if (++member.count > 150) { ws.close(1008, 'rate limit'); return; }
    member.lastSeen = Date.now(); ws.serializeAttachment(member);
    const room = await this.info();
    if (!room) { ws.close(1000, 'room expired'); return; }
    let data; try { data = JSON.parse(message); } catch { ws.close(1008, 'invalid JSON'); return; }
    if (data.type === 'ping') { ws.send(JSON.stringify({type: 'pong'})); return; }
    if (member.role === 'host' && data.type === 'update') {
      room.metadata = metadata(data); await this.ctx.storage.put('room', room); await this.broadcast(); return;
    }
    if (member.role === 'host' && data.type === 'kick') {
      const target = this.sockets().find(s => s.deserializeAttachment().id === data.to && s !== ws);
      if (target) { await this.ctx.storage.delete('session:' + target.deserializeAttachment().hash); target.close(1008, 'removed by host'); }
      return;
    }
    if (data.type !== 'signal' || !['offer', 'answer', 'ice'].includes(data.kind) || typeof data.payload !== 'object') return;
    const to = member.role === 'host' ? data.to : 'host';
    const target = this.sockets().find(s => s.deserializeAttachment().id === to && s !== ws);
    if (target) target.send(JSON.stringify({type: 'signal', from: member.id, kind: data.kind, payload: data.payload}));
  }
  async webSocketClose(ws, code, reason) {
    const identity = ws.deserializeAttachment();
    if (identity?.hash && !this.sockets().some(other => other !== ws && other.deserializeAttachment().id === identity.id)) await this.ctx.storage.delete('session:' + identity.hash);
    try { ws.close(code === 1006 ? 1001 : code, reason); } catch {}
    if (identity?.role === 'host' && !this.host()) for (const peer of this.sockets()) { peer.send(JSON.stringify({type: 'host-offline'})); peer.close(1001, 'host offline'); }
    await this.broadcast();
  }
  async webSocketError(ws) { await this.webSocketClose(ws, 1011, 'connection error'); }
  async alarm() {
    const room = await this.ctx.storage.get('room');
    if (!room) {
      const rooms = (await this.ctx.storage.get('rooms') || []).filter(r => r.expiresAt > Date.now());
      await this.ctx.storage.put('rooms', rooms);
      if (rooms.length) await this.ctx.storage.setAlarm(Math.min(...rooms.map(r => r.expiresAt)));
      return;
    }
    if (room.expiresAt <= Date.now()) { for (const ws of this.sockets()) ws.close(1000, 'room expired'); await this.ctx.storage.deleteAll(); return; }
    for (const ws of this.sockets()) if (Date.now() - ws.deserializeAttachment().lastSeen > 75000) ws.close(1001, 'heartbeat expired');
    await this.ctx.storage.setAlarm(this.sockets().length ? Math.min(Date.now() + 60000, room.expiresAt) : room.expiresAt);
  }
}

function invitePage() {
  return new Response(`<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>加入 Blocklink 房间</title><style>body{background:#f2f0e7;color:#243a30;font:18px system-ui;max-width:560px;margin:15vh auto;padding:24px}h1{font-size:40px}p{line-height:1.8}button{font:inherit;background:#315b42;color:white;border:0;border-radius:10px;padding:14px 24px;cursor:pointer}small{display:block;margin-top:28px;color:#667469}</style><h1>一起进入方块世界</h1><p>在 Blocklink 中粘贴此邀请链接，即可查看房间、同步 Mods 并加入游戏。</p><button id="copy">复制邀请链接</button><p id="status" role="status"></p><small>房主需要保持电脑与 Blocklink 后台在线。请仅向信任的朋友分享邀请。</small><script>document.getElementById('copy').onclick=async()=>{try{if(!location.hash)throw Error();await navigator.clipboard.writeText(location.href);document.getElementById('status').textContent='已复制，请在 Blocklink 的「加入房间」中粘贴。'}catch{document.getElementById('status').textContent='请复制浏览器地址栏中的完整邀请链接。'}};</script></html>`, {headers: {'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer', 'Content-Security-Policy': "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'"}});
}
