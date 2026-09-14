# 开始使用 / Getting started

## Windows

1. 在官网下载 Windows 完整压缩包，解压后打开 Blocklink.exe。需要 Windows 10/11 x64 和系统 WebView2。
2. 在启动器设置选择简体中文或 English，然后选择玩家档案。当前本地玩家不支持需要正版验证的服务器；微软登录尚未获得 Minecraft API 访问资格。
3. 选择「新建游戏」或「整合包」，选好想玩的内容。所需 Java、游戏和 Loader 会自动准备。
4. 点「开始游戏」。需要处理无法适配的内容时，启动器会让你确认方案。

Download and extract the full Windows ZIP, then open Blocklink.exe. Choose your language in Launcher settings and select a player. Add a game or modpack and press Play; required files are downloaded automatically. The current local-player mode cannot join servers requiring authenticated accounts.

## macOS / Linux 预览包 / Native previews

在 [GitHub Releases](https://github.com/marcusyang-meta/blocklink/releases) 按电脑系统与芯片选择原生包。macOS 解压后打开 Blocklink.app，或使用 DMG；Linux 解压后给 Blocklink.AppImage 添加执行权限再打开，也提供 DEB。Linux 需要兼容的桌面环境和 Secret Service 凭据库。

这些预览版未签名，macOS 尚未公证。构建与后台启动测试已通过，但各平台的实际 Minecraft 窗口、声音、输入和光影仍待实机验收。

Choose the native package for your OS and processor on GitHub Releases. On macOS, extract the app ZIP or use the DMG. On Linux, make the extracted AppImage executable, or install the DEB. These unsigned previews passed native build and service tests; Minecraft graphics, audio, input and shader testing is still pending. macOS notarization is not configured.

## 带上已有存档 / Bring your worlds

关闭原游戏，在游戏的「世界与存档」选择导入，选择世界文件夹、saves 或 .minecraft。确保目标游戏的版本、Loader 和 Mods 与原世界匹配。导入创建独立副本，原世界保留。

Close the source game. In Worlds & saves, import a world folder, saves folder or .minecraft directory. Match the original game version, loader and mods first. Import creates a separate copy and preserves the original.

## 和朋友一起玩 / Play together

服务器在房主自己的电脑上运行。创建服务器或部署已有世界，阅读并同意 Minecraft EULA 后启动服务器，开启房间并把邀请发给朋友。朋友也需要 Blocklink，粘贴邀请后同步玩法并加入。房主需要保持电脑及后台服务在线。房间服务需要可用的 Cloudflare 大厅配置；见 lobby/README.md。

Servers run on the host's computer. Create a server or deploy a world, accept the Minecraft EULA and start it. Open a room and send its invitation to friends using Blocklink. They paste it to sync and join. Keep the host computer and background service online. Hosting requires a configured Cloudflare lobby; see lobby/README.md.

## 更新与备份 / Updates and backups

退出启动器后替换应用文件即可更新；游戏数据保留在系统用户数据目录。窗口关闭后后台可能仍在运行，无法覆盖 EXE 时先另存新版，避免强制结束正在保存的服务器。永久删除游戏会同时删除该游戏的世界和本地备份。重要世界请另存独立备份。

Replace the app files to update; game data stays in the OS user-data directory. A background host may continue after closing the window. If the executable is locked, save the new version separately rather than force-stopping a server that may be saving. Permanently deleting a game also deletes its worlds and local backups. Keep independent backups.

## 反馈 / Feedback

通过 https://github.com/marcusyang-meta/blocklink/issues/new/choose 提交问题。请附版本、系统和复现步骤，不要附账户令牌或私人邀请链接。

Report issues with your app version, OS and reproduction steps. Do not include account tokens or private invitations.
