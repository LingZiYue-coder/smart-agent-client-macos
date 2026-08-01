<p align="center">
  <img src="src-tauri/icons/128x128.png" width="88" alt="Smart Agent" />
</p>

<h1 align="center">Smart Agent for macOS</h1>

<p align="center">
  面向 macOS 的 Codex 一键接入客户端。登录、连接、模型、余额与恢复入口，都在一个桌面应用中完成。
</p>

<p align="center">
  <a href="https://www.smart-agent.eu.cc/">官方网站</a>
  ·
  <a href="https://platform.smart-agent.eu.cc/">开放平台</a>
  ·
  <a href="https://github.com/LingZiYue-coder/smart-agent-client-macos/actions/workflows/build-dmg.yml">下载构建产物</a>
</p>

---

## 产品能力

- 自动检测 Codex CLI 与 macOS 桌面应用
- 一键写入、保持与恢复 Codex 配置
- 通过 Smart Agent 账户统一管理模型、余额和使用记录
- 支持菜单栏托盘运行，关闭窗口后保持后台服务
- 同一个 DMG 同时支持 Apple Silicon 与 Intel Mac

## 下载

打开仓库的 **Actions → Build macOS DMG**，选择最新成功记录，在 Artifacts 中下载：

```text
Smart-Agent-macOS-universal-*
```

当前自动构建使用 ad-hoc 签名，首次打开时 macOS 可能要求在“系统设置 → 隐私与安全性”中确认。面向公众发布时，应配置 Apple Developer ID 签名与公证。

## 本地开发

需要 macOS 12 或更高版本、Node.js 22、pnpm、Rust stable 和 Xcode Command Line Tools。

```bash
pnpm install --frozen-lockfile
pnpm tauri dev
```

生成通用 DMG：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build \
  --target universal-apple-darwin \
  --bundles dmg \
  --config src-tauri/tauri.macos.conf.json
```

## 安全与隐私

Smart Agent 仅在完成账户登录、Codex 配置和用户主动操作时访问必要数据。请勿在 Issue、日志或截图中提交 API 密钥、访问令牌、账号密码或本地 `~/.codex` 内容。

## License

[MIT](LICENSE) © 2026 Smart Agent

---

## English

Smart Agent for macOS is a one-click Codex access client. It brings sign-in, connection setup, model selection, balance, usage records, and recovery into one desktop application.

The GitHub Actions workflow builds a universal DMG for both Apple Silicon and Intel Macs. See the Chinese sections above for build instructions and security notes.
