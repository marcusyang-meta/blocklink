# Contributing / 参与开发

Use GitHub Issues for reproducible bugs and feature ideas. Include the Blocklink version, OS, Minecraft version and loader when relevant. Explain what you expected and what happened. Remove tokens, private invitations and personal paths before posting logs.

欢迎通过 GitHub Issues 提交问题和建议。请附启动器版本、系统，以及相关的 Minecraft / Loader 版本，说明复现步骤。不要公开令牌、私人邀请或含个人路径的完整日志。

For code changes, keep the scope focused, explain the resulting behavior and list the checks you ran. UI changes need both Chinese and English text and a screenshot. Do not translate user names or publisher content. See desktop/I18N.md.

Run `pnpm build` and `node --test test/i18n.test.mjs` in desktop/, and `cargo test --workspace --locked` from the root. For lobby changes, run `pnpm install --frozen-lockfile` and `pnpm test` in lobby/. Platform-specific changes must say which systems were actually tested.

Never commit local game data, Microsoft credentials, Cloudflare tokens, .dev.vars files or service.json. Keep original worlds safe when testing migration and deletion; use isolated test directories.
