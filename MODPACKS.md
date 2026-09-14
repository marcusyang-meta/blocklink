# 整合包

在顶部「整合包」中搜索 Modrinth 项目、选择版本并安装；也可以点击「导入整合包」。每次安装创建独立游戏实例，自动安装指定 Minecraft、Fabric / Quilt / Forge / NeoForge 和托管 Java。可选择是否包含作者标记的可选客户端文件。

导入包含清单文件、公共 overrides 和 client-overrides（客户端层优先），包括配置、资源包、光影等；跳过 server-only 文件和 server-overrides。模组进入共享内容仓库，实例使用链接或克隆。整合包实例启动时不自动替换作者锁定的模组版本；手动修改仍可在模组管理中进行。

下载按 SHA-512、SHA-1 和发布大小校验，支持缓存、HTTPS 备用下载地址。安装先写入同卷临时实例，全部内容导入成功后发布。路径穿越、符号链接、危险 Windows 文件名、重复文件和大小写冲突会被拒绝；压缩包最多 512 MB，清单下载合计最多 8 GB，覆盖文件解压最多 4 GB。支持 Modrinth CDN 及格式推荐的 GitHub/GitLab 下载来源，重定向仍受 HTTPS 与域名限制。

下载任务显示当前文件的实际字节进度、速度，并支持取消和重试。取消在下载块或处理阶段边界生效，正在等待网络响应时可能需要等待超时。已校验缓存保留。内容发布后的游戏环境安装若中断，保留完整整合包实例；继续安装或在该实例中修复即可完成运行环境。任务历史暂保存在本次后台运行期间。

启动失败页面提供重新启动、修复游戏文件、运行设置、历史环境恢复入口和日志。诊断提示是本地日志规则匹配，不能保证覆盖所有崩溃。历史环境恢复由用户选择现有快照，不将任意快照称为「已知可运行版本」。

当前支持 Modrinth `.mrpack` v1 和 HMCL / PCL 导出的完整 MCBBS ZIP（manifestVersion 1，mcbbs.packmeta 或含 addons 的 manifest.json，支持单一外层目录）。MCBBS 的 game/minecraft、Fabric、Quilt、Forge、NeoForge 组件自动匹配；overrides 中的模组、配置、资源包和光影保留，清单声明的本地文件按 SHA-1 校验。自定义启动库/参数、未知组件、远程缺失文件和 CurseForge 清单在导入前明确拒绝，避免静默丢内容。尚不支持任意启动器压缩目录、HMCL 服务器动态更新包、CurseForge 下载清单、整合包自动升级或把任意客户端整合包直接当作服务器包。现有世界部署和服务器功能继续独立使用。

## 验证

- 后台测试覆盖导入原子性、两实例复用相同模组、覆盖层优先级、路径穿越和大小写冲突、客户端内容选择、下载取消与任务单次重试。
- 在隔离数据目录从真实 Modrinth 安装 Fabulously Optimized 8.2.0 / Minecraft 1.21.4 / Fabric 0.19.3。中途取消，再继续安装成功，46 个模组进入实例，配置和资源包存在，启动检查通过。
- 同一 `.mrpack` 从本地再次导入成功，46 个模组全部链接复用；游戏日志确认完成初始化并正常退出。

格式参考：[Modrinth 官方整合包格式](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack)。

## 分类和筛选

玩法分类来自 Modrinth 整合包分类列表；可组合游戏版本、Fabric/Forge/NeoForge/Quilt 与下载量、相关度、最近更新、最新发布、收藏量排序。选中项目后继续使用版本和加载器条件，默认只显示正式版，可主动打开测试版。已安装的在线整合包显示标记和打开入口。本地 .mrpack 使用同一个安装确认页面。

分类与筛选使用 [Modrinth 搜索接口](https://docs.modrinth.com/api/operations/searchprojects/)。

## 永久删除

实例与服务器的删除对话框默认执行永久删除，包含该实例的存档、配置与本地备份，不进入系统或 Blocklink 回收站。需输入实例名称确认，运行中或世界仍被占用时拒绝删除。移除联机房间与绑定，保留其他实例和共享下载缓存。旧版回收站数据不会自动清空。

删除中断时，未完成文件暂存在内部 deleting 目录，无法恢复为实例；下次启动继续清理。若仍被外部程序占用，需要先关闭占用程序。测试覆盖名称确认、世界锁、共享硬链接保留、备份删除和中断恢复。

## 本轮验证

39 项服务测试通过（7 项联网集成测试未重跑）。MCBBS ZIP 实际导入 46 个现有真实模组，中文配置保留，指定游戏及 Fabric 安装完成；检查缺文件、路径越界、大小写冲突、损坏校验和未开通来源，不发布残缺实例。

CurseForge 下载现在需要来源认证，尚未获得 Blocklink 的 API 访问配置，不提供虚假的在线来源入口。官方说明：https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/
