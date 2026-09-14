# 游戏版本与 Mod Loader

创建实例或托管服务器时，可选择 Vanilla、Fabric、NeoForge、Forge、Quilt。

- 游戏版本来自 Mojang 官方版本清单，显示 Minecraft 1.13 及之后的正式版，包括新的 26.x 命名。没有将版本上限写死；快照、预发布和 1.12.2 及更早版本不在此次支持范围内。
- Java 主版本读取游戏官方元数据，自动下载对应平台的 Temurin；缺少 JRE 包时尝试 JDK，也可在实例设置中指定 Java 路径。支持 Java 8 的检查流程，并保留 Java 9+ 模块镜像检查。
- Fabric 通过官方元数据服务查询、安装。
- NeoForge 从 Minecraft 1.20.2 开始，使用官方 Maven 版本清单，默认选择匹配游戏的最新稳定版，也允许明确选择列表中的测试版。1.20.1 的旧 NeoForge 坐标不支持，不把 Forge 当作 NeoForge。
- NeoForge 安装器从官方 Maven 下载并验证 SHA-1，只对 Blocklink 管理的目录运行；客户端补丁安装到内部 runtime，服务器生成自己的参数文件。详细安装日志保存在实例目录的 installer.log。
- NeoForge 服务端通过官方安装器生成的 win_args.txt / unix_args.txt 启动，不把它当作普通 server.jar。
- Modrinth 的搜索、版本、依赖解析都按当前 Loader 筛选；本地导入读取对应 Fabric / Quilt JSON 或 Forge / NeoForge TOML。Quilt 本地导入也识别 Fabric 描述；Modrinth 下载只选择明确标记 Quilt 兼容的版本。服务器部署、发布环境、共享存储、启动前同步继续使用完整游戏版本与 Loader 版本校验。
- 已绑定本机服务器的实例启动时自动连接该服务器；1.13—1.19 使用旧版连接参数，1.20+ 使用 Quick Play。1.16.5 的离线档案多人游戏在本次验收中受限，自动入服未通过；不能把所有旧版列为已验证离线联机。

游戏版本出现在列表中代表可以走安装流程，不代表已完成每个版本、每个平台的实际游戏验收。Windows 的具体运行测试见 VALIDATION.md；macOS/Linux 仍需实机验收。旧版在 ARM 平台可能没有官方可用的本机库。

升级前先备份世界，已有实例不会自动升级游戏或 Loader。Mod 文件必须适用于该 Loader；不能通过改名把 Fabric/Forge 文件变成 NeoForge Mod。

Forge 和 Quilt：

- Forge 从官方 Maven 查询当前 Minecraft 的精确版本列表，按数字排序，使用带游戏版本的坐标下载并校验官方安装器。安装器的游戏继承关系必须匹配。现代服务器读取平台参数文件，旧安装器生成的 Forge 启动 JAR 也有对应入口。
- Quilt 使用官方 Meta 客户端配置及官方服务器安装器；安装器下载使用 Maven 随附的 SHA-256 校验值。0.15.1 的 Meta API 哈希与 Maven 文件不同，本轮确认实际文件匹配 Maven 校验值后，改用同源仓库校验文件，未关闭校验。
- Quilt 的按游戏查询接口也会返回历史 Loader；26.x 在本启动器中最低使用 0.30.1，默认选最新稳定版，也可主动选择更新测试版。此基线是本启动器的支持范围，不代表上游所有历史兼容情况。
- Forge 与 NeoForge 始终是不同环境；发布、邀请和同步锁定精确的 Loader 家族与版本。朋友双方需要更新 Blocklink 才能识别新 Loader。
- 26.2 的真实 Windows 验收包括 Forge 65.1.3 + GeckoLib 5.5.5、Quilt 0.30.1 + Lithium 0.25.3，以及前一轮 NeoForge 26.2.0.87；全部通过实际玩家入服。不同 Loader 的世界与 Mod 不能任意互换。
