# Microsoft 登录接入状态

2026-09-13：已在用户目录注册 Blocklink，Client ID 为 a8080fd5-fcd6-4dbf-811f-91d0f10759c9，支持个人 Microsoft 账户，已启用 public client flows。已接入本机配置并内置于后续构建，微软设备验证码签发成功；目前正在等待真实账户授权和 Minecraft 档案校验。设备代码登录、Xbox/XSTS、Minecraft 档案查询、系统凭据库和刷新流程已经实现；这不代表应用已获 Minecraft API 访问资格。

产品发布者需要为 Blocklink 注册自己的公共客户端应用，支持个人 Microsoft 账户，并启用公共客户端流程。当前使用 consumers 设备代码端点，scope 为 XboxLive.signin offline_access，不需要在桌面程序内保存 client secret。

Blocklink 正式构建默认使用自己的上述公共 Client ID。分叉项目应注册自己的应用，构建前设置 BLOCKLINK_MICROSOFT_CLIENT_ID 覆盖。新装与旧数据目录在 clientId 为空时均自动采用内置有效 UUID；已有自定义配置保留。也可以在启动器设置的「高级设置」录入产品应用 ID。普通玩家页面不显示开发者参数。

接入验收需要真实完成设备代码授权、Xbox/XSTS、Minecraft profile，再验证启动和刷新。OAuth 授权取消/过期、应用配置错误、Minecraft 拒绝访问、未创建游戏档案现在分别返回可读提示，不显示授权响应里的凭据。

Microsoft 官方注册文档仍列出 Azure 账户、订阅与目录权限要求：
https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app

Minecraft API 申请入口由 minecraft-launcher-lib 项目文档指向 https://aka.ms/mce-reviewappid 。本次能解析到微软表单链接，但不能据此确认申请目前是否受理、需要多久或已批准。旧的 Minecraft 帮助文章现在只返回空壳页面。不能把普通微软登录成功当作 Minecraft API 授权成功。

参考：https://minecraft-launcher-lib.readthedocs.io/en/stable/tutorial/microsoft_login.html

实际设备代码接口为个人账户返回 https://www.microsoft.com/link；不能把验证入口写死为 microsoft.com/devicelogin。界面现使用响应中的 verification_uri。
