# KeyDock

KeyDock 是一个常驻桌面的登录启动器：点击「站点 + 账号」，自动拉起一个全新的临时浏览器完成登录填充，提交后完全放手；会话结束即销毁，不留痕迹。

- 产品与技术决策以交接文档为准，不在实现中重新讨论。
- 当前进度：**v0.1.0**，Phase 0–6 的 Windows 部分已完成；macOS 硬件相关项待发布后人工验证。
- 实施报告：[docs/implementation-report.md](docs/implementation-report.md)
- 人工测试清单：[docs/manual-test-checklist.md](docs/manual-test-checklist.md)
- Phase 0 详情：[docs/phase-0-report.md](docs/phase-0-report.md)
- 架构决策：[docs/adr/README.md](docs/adr/README.md)

## 技术栈

- Tauri 2.12 + Rust
- React 19 + Vite 7 + TypeScript 5.9
- Tailwind CSS v4
- shadcn/ui（Phase 2 接入）
- Chromium CDP：手写薄客户端（`tokio-tungstenite` + `serde_json`）

## 目录

```
crates/keydock-core      领域模型、配置、凭据库、边界纯函数（Phase 1）
crates/keydock-browser   CDP 客户端与 fixture spike（Phase 0/3）
src-tauri                Tauri 壳（Phase 2 起接入窗口/托盘/命令）
src                      React 前端（Phase 2 起写 UI）
spikes/mouse-hook        Windows Mouse4 钩子与覆盖层 spike
fixtures/react-form      真实 React 19 受控表单测试页
docs/adr                 架构决策记录
```

依赖方向：`src-tauri → keydock-browser → keydock-core`。两个 crate 都不依赖 Tauri。

## 环境要求

- Windows 10/11 x64 或 macOS 12+ Apple Silicon
- Node.js 24+、pnpm 10+
- Rust stable（MSVC toolchain on Windows）
- Chromium 内核浏览器（Chrome / Edge / Chromium / Brave；Phase 0 实测 Chrome 155）
- WebView2（Windows）

## 常用命令

```powershell
pnpm install
pnpm test                  # TypeScript 类型检查
pnpm build                 # Vite 构建 + 生成内嵌 changelog
cargo test --workspace
cargo clippy --workspace --all-targets
pnpm tauri dev             # 桌面开发

# 生产安装包（Windows）
$env:TAURI_SIGNING_PRIVATE_KEY = "F:\Rea_temp\KeyDock\keydock-updater.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
pnpm tauri build --bundles nsis

# CDP / OCR / 鼠标 spike
pnpm fixtures:build
cargo run -p keydock-browser --example cdp_spike
cargo run -p keydock-browser --example captcha_eval -- "F:\Rea_temp\DevDock\captcha-samples"
cargo run -p keydock-mouse-spike
```

spike 报告默认写到 `F:\Rea_temp\KeyDock\phase0\`，可用 `KEYDOCK_PHASE0_DIR` 覆盖。
临时浏览器 Profile 位于该目录的 `sessions/` 下，验证结束后删除。

## 安全约束（摘要）

- 不抓取、不记录、不截图任何密码或页面内容；唯一的密码明文出口是 `shared_get`。
- 临时 Profile 每次登录新建，结束或下次启动时清除。
- 不发送 `Emulation.setDeviceMetricsOverride`，不注入 `--enable-automation`。
- Phase 0/验证过程禁止截图、录屏、屏幕捕获；状态一律用 DOM 文本、Win32 状态和日志判断。

## 平台状态

| 项 | Windows | macOS arm64 |
|---|---|---|
| CDP 登录（普通/iframe/OCR） | 已验证 | 代码与 CI 就绪，待真机 |
| Mouse4 钩子 + 径向菜单 | 已验证 | CGEventTap 待真机 |
| Windows Hello / Touch ID | 可用性已验证 | 待真机 |
| 空闲锁定 / 系统锁屏锁定 | WTS 实测 | 待真机 |
| 托盘 / 快捷键 / 自启 | 已实现，部分待人工 | 待真机 |
| NSIS / DMG | NSIS 已构建 | CI 构建 |

macOS 专属能力按用户要求跳过实机验证，统一记录在 `docs/manual-test-checklist.md`；不得把未执行项标记为通过。
