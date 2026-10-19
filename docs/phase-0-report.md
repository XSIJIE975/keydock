# Phase 0 报告

- 日期：2026-10-20
- 范围：§12.1 Phase 0、§7.2 四项判据、Windows 鼠标钩子 spike
- 结论：**Windows 部分完成**；macOS 鼠标钩子因无测试机仍为 Pending。

## 交付物

| 交付物 | 位置 | 状态 |
|---|---|---|
| Rust workspace | `Cargo.toml`、`crates/keydock-core`、`crates/keydock-browser` | 编译通过 |
| Tauri 2 壳 | `src-tauri/` | `cargo check -p keydock` 通过 |
| React 19 + Vite 7 + Tailwind v4 骨架 | `src/`、`vite.config.ts`、`package.json` | `pnpm build` 通过 |
| 薄 CDP 客户端 | `crates/keydock-browser/src/cdp/` | 14 项检查全过 |
| CDP fixture | `crates/keydock-browser/examples/cdp_spike/`、`fixtures/react-form/` | 可复现 |
| chromiumoxide 判据探针 | `crates/keydock-browser/examples/chromiumoxide_probe.rs` | 已执行 |
| Windows 鼠标 spike | `spikes/mouse-hook/` | 自测全过 |
| CDP ADR | `docs/adr/0001-cdp-driver.md` | 完成 |
| 鼠标 ADR | `docs/adr/0002-mouse-hook.md` | Windows 完成、macOS Pending |

## 验证结果

### 构建与单测

```text
pnpm build                              PASS
cargo check -p keydock                  PASS
cargo test -p keydock-core              PASS (1/1)
cargo test -p keydock-mouse-spike       PASS (5/5)
```

### CDP

`cargo run -p keydock-browser --example cdp_spike`：14/14 PASS。

覆盖：

- 启动、`DevToolsActivePort`、WebSocket
- 启动参数无 `--enable-automation` / 视口覆盖
- flat `sessionId` 路由（两个 target）
- 普通表单 + trusted 点击
- 真实 React 19 受控表单
- 跨域 iframe + 隔离 execution context
- 跨域 SSO 成功
- 密码错误不误判成功
- 下载 + `Browser.downloadProgress`
- 延迟 SPA
- 窗口缩放重排
- 关闭 + 临时 Profile 删除

### 鼠标钩子

`cargo run -p keydock-mouse-spike`：

- 短按 81ms：目标窗口收到 1 down + 1 up，原事件被吞掉。
- 长按 450ms：覆盖层出现，前台窗口不变，选择扇区 0；负坐标副屏选择扇区 2。
- 双屏定位：`-2880..0` 与 `0..3840` 两块屏，192/144 DPI；副屏覆盖层位于 `-1620..-1260`，且完全在工作区内。
- 修复测试目标置前逻辑后，连续 3 次自测全过。
- 5 个纯几何单测全过。

## 关键决策

1. **CDP 使用手写薄客户端**，不采用 `chromiumoxide` 0.9.1。
   理由：即使 `disable_default_args()`，它仍把页面视口固定为 800×600
   （违反“不发送 Emulation override”），并强制注入 `--disable-extensions`。
   详见 ADR 0001。
2. Windows 鼠标钩子必须从 `MSLLHOOKSTRUCT.mouseData` 高字取 XButton，不能从 `wParam` 高字取。
3. 短按补发不能在钩子回调内 `SendInput`；改为 PostMessage 到主线程后异步补发。
4. 产品 UI 代码尚未开始；`src/main.tsx` 仅为空的 Vite 挂载点。

## 未完成 / 风险

| 项 | 状态 | 处理 |
|---|---|---|
| macOS CDP spike | 未执行 | 需要一台 arm64 Mac；代码已按 macOS 路径条件编译，未验证 |
| macOS CGEventTap / 辅助功能授权 | 未执行 | Phase 5 前必须有 Mac 测试机，否则径向菜单不能宣称双平台 |
| Tauri 真实覆盖层 focusable(false) | 源码核对通过；cargo 直跑 example 遇 STATUS_ENTRYPOINT_NOT_FOUND，主壳可启动 | Phase 2 用 `pnpm tauri dev` 冒烟确认 |
| Edge / 其他 Chromium 内核 | 未测试 | Phase 3/4 |
| 360 / QQ 调试端口 | 未测试 | Phase 4 按版本实测 |
| OCR | 未开始 | Phase 3；本 Phase 不涉及 |
| 安全软件拦截低级钩子 | 未测试 | Phase 5 保留快捷键等价入口 |
| WebView2 数据目录 | 未配置 | Phase 2 创建 webview 时显式指定 `data_directory` |
| Git / GitHub 远端 | 未初始化 | 等待仓库创建与首次提交的明确授权；当前只落盘了工作区文件 |

## 下一阶段（Phase 1）入口

实现 `keydock-core`：

1. `paths.rs`：数据目录、写探测、启动清扫。
2. `config.rs`：加载、归一化、保存、热载、分组合并。
3. `vault.rs`：scrypt、AES-256-GCM、固定测试向量、原子写、错误延迟。
4. `bounds.rs`：窗口边界纯函数，覆盖负坐标与混合 DPI。
5. `models.rs`：补齐配置结构与会话模型。
6. `selector.rs`：继承/空串关闭语义。

验收：单测全绿，vault 固定向量通过；不接 Tauri、不写 UI。

## 复现报告

临时产物统一位于 `F:\Rea_temp\KeyDock\phase0\`：

- `cdp-spike-report.json`
- `chromiumoxide-probe.json`
- `mouse-hook-report.json`
- 对应运行日志

原始 fixture 与源码都在仓库内；临时 Profile 和下载文件已在验证后删除。
