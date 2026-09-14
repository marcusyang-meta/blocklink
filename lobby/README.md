# Blocklink Cloudflare 大厅

## 官网

`public/` 与大厅一起部署：`/` 为中文首页，`/en/` 为英文首页，`/privacy` 和 `/en/privacy` 为对应隐私说明。页首语言切换保留章节锚点，不使用 Cookie 或本地存储；桌面启动器尚未因此变为双语。

修改文案时同步更新两种语言的 HTML。样式与语言切换脚本位于 `public/assets/`。发布新 Windows 构建时同步替换 `public/downloads/` 中的 EXE、完整 ZIP 与 SHA256SUMS.txt，并更新两种语言的下载大小和状态。保留 ZIP 中的第三方许可。

部署前运行 `node --test test/site.test.mjs test/room.test.mjs`，并检查两种语言的首页、隐私页、章节跳转与窄屏布局。静态资源和邀请/API 路由分开处理。

这个 Worker 是启动器的公网会合点。房主与朋友分别向它建立出站 HTTPS/WebSocket 连接，所以双方处于内网也可以找到同一个房间。

- Workers：房间 API 与邀请说明页。
- SQLite Durable Objects：每个房间独立保存邀请、成员、心跳、有效期，并转发 WebRTC 信令。
- Cloudflare TURN：只在无法直连时承担游戏/Mod 流量。长期 TURN 密钥仅保存在 Worker secrets。
- Rust 后台：原生 WebRTC 加密连接，可靠 DataChannel 内使用 Yamux 多路复用游戏与 Mod 数据流；Mod 下载仍通过 SHA-512 校验并进入共享仓库。

默认私有邀请制，最多 20 个房间，每房间最多 8 位朋友，12 小时过期。不提供公开房间搜索。房主关电脑或退出后台时离线；关闭启动器窗口不会退出后台。房间重开需要新邀请。

## 部署

需要 Node.js、pnpm 与自己的 Cloudflare 账户。此目录与桌面应用单独部署。

```sh
pnpm install --frozen-lockfile
pnpm exec wrangler login
pnpm exec wrangler secret put LOBBY_HOST_KEY
pnpm exec wrangler secret put TURN_KEY_ID
pnpm exec wrangler secret put TURN_KEY_API_TOKEN
pnpm deploy
```

`LOBBY_HOST_KEY` 是你生成的随机开房凭据；仅分给允许开房的人。普通朋友只需要邀请链接。不要把 Cloudflare 账户 API Token 当成开房凭据。

TURN_KEY_ID 和 TURN_KEY_API_TOKEN 来自 Cloudflare Realtime TURN 的专用密钥。生成接口：
https://developers.cloudflare.com/realtime/turn/generate-credentials/

部署成功后，将实际的 `https://blocklink-lobby.<你的子域>.workers.dev/` 地址与开房凭据填入启动器设置。开房凭据保存在系统凭据库。可以通过 Wrangler 配置自有域名。不要把 `.dev.vars`、`.wrangler`、API Token 或本机数据目录加入发布包。

首次部署并不等于真实 TURN 验收：需用两台不同网络的电脑测试正常连接，再强制只走 TURN 测试。Workers/DO 与 TURN 分别计费，免费额度不代表永久免费或无限带宽。部署者应在 Cloudflare 控制台设置用量通知并检查实际账单。

## 本机验收

```sh
pnpm test
node test/local-server.mjs
# 在另一个终端，从 Rust 工作区执行：
cargo test -p blocklink-service lobby::tests::cloudflare_room_mod_sync_tcp_and_revocation -- --ignored --nocapture
```

测试服务仅监听 127.0.0.1:8787，使用固定的无价值测试凭据，`LOCAL_TEST=true` 只允许返回空 ICE 配置以测试本机直连。生产配置没有该开关；未设置 TURN secrets 时开房连接会报错，不会声称中继已可用。

测试邀请不能供公网朋友使用。游戏转发只允许访问房主选定服务器的本机游戏端口；文件传输只允许读取已发布、允许客户端使用的 Mod。大厅不持有世界存档，也不能发送服务器控制台命令。

## 已部署实例
本次大厅：https://blocklink-lobby.junjie-f33.workers.dev/ 。TURN 已配置，原生强制中继测试通过。双方须使用包含就绪握手修复的同一新版启动器。测试模式可设 BLOCKLINK_TEST_LOBBY_URL 为公网根地址、BLOCKLINK_TEST_RELAY_ONLY=1；开房凭据从系统凭据库读取。

