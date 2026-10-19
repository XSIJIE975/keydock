# KeyDock v0.1.0 实施报告

- 日期：2026-10-20
- 范围：交接文档 Phase 1–6
- 平台：Windows x64 已实测；macOS arm64 代码与 CI 已就绪，硬件相关项待真机人工验证

## 1. 完成度

| Phase | 内容 | 状态 |
|---|---|---|
| 0 | CDP spike + Mouse4 spike | Windows 完成；macOS 待真机 |
| 1 | keydock-core：paths/config/vault/bounds/models/selector | 完成，41 个单测通过 |
| 2 | Tauri 壳 + React UI：球/面板/锁屏/站点/账号/设置 | 完成，真实 WebView2 冒烟通过 |
| 3 | CdpDriver + 登录状态机 + 验证码四模式 + 下载 | 完成，普通/iframe/OCR 端到端通过 |
| 4 | 书签、浏览器探测、选择器学习、清理 | 完成，真实书签与学习流程实测 |
| 5 | 径向菜单、快捷键、托盘、自启、空闲锁、生物识别、更新 UI | Windows 完成；Touch ID / CGEventTap 待真机 |
| 6 | 打包、CI、changesets、发版脚本 | NSIS 本地构建 + CI/Release 工作流就绪 |

## 2. 架构落地

```
src-tauri ──► keydock-browser ──► keydock-core
    └────────────────────────────► keydock-core
```

- `keydock-core`：纯 Rust，无 Tauri/GUI 依赖。
- `keydock-browser`：手写 CDP 客户端、浏览器探测、书签解析、OCR、登录/学习状态机。
- `src-tauri`：命令层、窗口、托盘、快捷键、径向钩子、空闲/会话锁、平台能力。
- `src`：React 19 + Vite 7 + Tailwind v4，shadcn 风格本地组件。

依赖方向没有循环；两个 crate 都不依赖 Tauri。

## 3. 安全实现

### 3.1 凭据库

- scrypt：`log_n=14, r=8, p=1, keylen=32`，16 字节随机 salt。
- AES-256-GCM：12 字节随机 nonce，AAD = `keydock-vault-v1`，密文包含 16 字节 tag。
- 外层 JSON 信封与交接文档 §4.2 一致。
- 原子写：临时文件 → `sync_all` → rename。
- 未解锁时 `has()` 返回 false，读取返回 `None`，写入返回 `Locked` 错误。
- 修改主密码 = 校验旧密码 → 新 salt + 新 key → 整体重封。
- 工号变更时迁移 `cred:<empNo>`；删除账号时清理凭据。
- 内存中的明文密码在结构体 Drop 时 zeroize；序列化明文在加密后 zeroize。

固定向量单测：

- RFC 7914 scrypt 向量（N=1024, r=8, p=16, dkLen=64）。
- NIST AES-256-GCM 全零向量（ct + tag）。
- 错误密码、锁定语义、改密、工号迁移、密文不含明文、无残留 `.tmp`。

### 3.2 快速解锁

- 派生密钥 base64 只写入系统钥匙串：Service `com.xsjie.keydock`，Account `vault-key`。
- 仅在用户显式开启快速解锁后写入。
- Windows 使用 `UserConsentVerifier`（系统决定 PIN/指纹/人脸）；失败或取消回退主密码。
- 主密码错误延迟 1s → 2s → 4s → 8s。
- macOS Touch ID 返回明确的“待真机验证”，不伪装成功。

### 3.3 会话隔离

- 每次登录/学习/打开书签使用全新临时 Profile。
- 启动参数只包含显式列表，无 `--enable-automation`，无固定视口。
- Windows Job Object `KILL_ON_JOB_CLOSE`：应用崩溃时浏览器随 Job 关闭。
- 关闭顺序：`Browser.close` → 等进程退出 → 删除 Profile。
- 启动时按前缀清扫 `.tmp/sessions`。
- 应用退出时 `cancel_all`，等待最多 3 秒再退出。
- 单实例插件保证清扫时无并发会话。

## 4. 浏览器与登录

### 4.1 CDP

手写客户端（`tokio-tungstenite` + `serde_json`）：

- flat `sessionId` 路由，每个 tab 独立 session。
- `Runtime.evaluate` 支持指定 `executionContextId`。
- iframe：`Page.getFrameTree` + `Page.createIsolatedWorld`。
- 填值使用原生 `value` setter + `input`/`change`，兼容 React 受控组件。
- 提交使用 `Input.dispatchMouseEvent` trusted click；退路 `requestSubmit()`。
- 下载使用 `Browser.setDownloadBehavior` + `Browser.downloadProgress`。
- 不发送 `Emulation.setDeviceMetricsOverride`。

### 4.2 登录状态机

- 阶段：`starting → filling → submitting → success | need-manual | failed`。
- 成功判定：
  1. 配置了 `success` 选择器 → 等元素可见（主页面与 iframe 上下文都检查）。
  2. 提交后离开站点域 → 等回到站点 host，且密码框不再可见。
  3. 提交后仍在站点域 → 等 URL 变化，且密码框不再可见。
- 超时 60 秒 → `need-manual`，高亮 `captcha → password → username` 第一个存在的输入框。
- 人工/验证码模式最多等 10 分钟；浏览器被关闭或取消会立即结束。
- 登录成功后浏览器保持打开供用户使用，用户关闭窗口后清理 Profile；`login_cancel` 可主动结束。

### 4.3 验证码

- 四种模式：off / fixed / ocr / manual。
- OCR：`ort 2.0.0-rc.10` + `common_old.onnx`，模型与字符集内嵌，cargo feature `captcha-ocr` 默认开启。
- 预处理：CatmullRom 缩放到高 64；长度不符时使用文字包围盒裁剪 + 4px 白边重试。
- 识别结果按 `maxRetries` 刷新验证码重试；全部失败转人工。
- 取图使用页面资源字节（fetch/data URI/canvas），不使用截图 API；canvas 污染直接降级人工。

8 张参考样本实测：

| 预处理 | 正确数 |
|---|---:|
| 基线 | 6/8 |
| 基线 + 长度校验后包围盒裁剪 | 7/8 |

样本 6 `sff → sffy` 由裁剪修正；样本 2 `7esw` 仍为单字符错误。
端到端 fixture：识别出 `sffy` → 填入 → 提交 → `CAPTCHA_OK`。

## 5. 书签与选择器学习

- 读取 Chrome / Edge / Chromium / Brave 的 `Bookmarks` JSON，只读，不写入、不改时间戳。
- 递归文件夹、按 URL 去重、关键词过滤在前端执行。
- 实测读取到本机 Chrome 书签并正确展示分组路径。
- 选择器学习：
  - 临时 Profile 打开登录页。
  - 每 500ms 检查主页面与所有子 frame，只读取结构，不读取任何表单值。
  - 检测到密码框即记录 username/password/submit/captcha/frame 选择器。
  - 监听 5 分钟；保存时接受选择器并关闭窗口。
  - 实测：自动记录 `#username` / `#password` / `#submit`，未读取密码值。

## 6. UI 与交互

- 收起：72×72 透明悬浮球（球 56 + 阴影留白）。
- 展开：360×600 面板；贴边翻转与 clamp 使用纯函数，覆盖负坐标与混合 DPI。
- 面板：搜索、可折叠分组、站点列表、账号横条、底部 Tab/退出。
- 悬浮球可在设置中隐藏；隐藏后通过托盘或全局快捷键打开面板。
- 首次启动与更新后的更新日志弹窗（localStorage 记录已读版本）。
- 书签 Tab：搜索、关键词白名单、账号选择后打开并填表或只打开。
- 设置 Tab：账号、全局密码、分组、浏览器、验证码、安全、快捷键、径向菜单、启动、更新、日志、关于、配置导入导出。
- 首次运行三步：主密码 → 生物识别 → 添加站点。
- 锁屏：主密码、Windows Hello、二次确认重置。
- 不展示持久登录状态；只有进行中/成功/失败瞬时反馈。
- 视觉：深色优先、跟随系统、zinc 底 + `#2dd4bf` 强调色、系统字体、13px 基准、32px 控件。
- 不使用桌面毛玻璃；不使用截图或视觉回归。

## 7. 平台能力

| 能力 | Windows | macOS |
|---|---|---|
| 托盘 | 已实现 | 同一 Tauri API，待真机 |
| 开机自启 | 已实测开启/关闭 | 同一插件，待真机 |
| 自动更新 | 插件 + NSIS + `latest.json` | DMG，待真机 |
| 空闲锁定 | `GetLastInputInfo` 轮询 | 待实现（已列入人工清单） |
| 系统锁屏/休眠 | `WTSRegisterSessionNotification` + 电源事件；实测锁屏触发 | 待实现 |
| 生物识别 | `UserConsentVerifier`，可检测可用性 | 待真机 |
| 全局快捷键 | 插件，注册成功 | 同一插件，待真机 |
| 鼠标钩子 | `WH_MOUSE_LL`，短按补发 + 长按菜单已实测 | `CGEventTap` 待真机 |

径向菜单实测：

- Mouse4 长按 250ms 显示 360×360 覆盖层，8 个扇区。
- 短按在 Windows spike 中验证补发，目标窗口只收到一对事件。
- 覆盖层 `focusable(false)` + `ignore_cursor_events(true)`。
- 主屏东侧选择扇区 0 → 打开面板；副屏南侧选择扇区 2；负坐标屏幕定位正确。
- 8 扇区几何与多显示器定位有纯函数单测。

## 8. 实测证据

### 8.1 自动化检查

```text
pnpm build                 PASS
cargo test --workspace     PASS
  keydock-core             41 tests
  keydock-browser          16 tests (1 OCR sample test ignored by default)
  mouse geometry            5 tests
cargo clippy --workspace   PASS (无警告)
```

### 8.2 WebView2 端到端

通过 WebView2 CDP 读取 DOM 文本与结构（无截图）：

- 首次运行设置主密码 → 步骤 2 → 步骤 3 → 完成。
- 面板显示站点、分组、账号条。
- 锁屏 → 主密码解锁 → 面板恢复。
- 设置页全部区块渲染；浏览器探测显示 Chrome / Edge。
- 书签页读取真实 Chrome 书签。
- 普通表单登录：账号 900001、密码填充、`FIXTURE_OK`。
- 跨域 iframe 登录：子 frame 内填充并提交，`IFRAME_OK`。
- OCR 验证码：`sffy` 自动识别并提交，`CAPTCHA_OK`。
- 关闭浏览器后临时 Profile 目录为空。
- 配置导出/导入成功；开机自启 true→false 往返成功。
- 系统锁屏时 KeyDock 自动进入 locked 状态（WTS 实测）。
- Mouse4 长按径向菜单显示并选择“打开面板”，主窗口展开。

### 8.3 构建产物

- Windows NSIS 安装包由 `pnpm tauri build --bundles nsis` 产出（15.01 MiB，见 `target/release/bundle/nsis/`），并生成 updater `.sig`。
- 本地构建使用 `F:\Rea_temp\KeyDock\keydock-updater.key` 签名 updater 产物；私钥不进入仓库。
- WebView2 数据位于 `%LOCALAPPDATA%\com.xsjie.keydock`；配置文件与凭据库位于 `%APPDATA%\com.xsjie.keydock`。
- 静默安装冒烟因当前 Windows 会话进入锁屏而未能完成，已列入人工清单；安装包本身已由 Tauri NSIS 成功产出。
- macOS DMG 由 GitHub Actions `macos-latest` 构建，本地 Windows 无法产出。

## 9. 未完成与待人工验证

以下项目需要 macOS 真机、Windows Hello 真实凭据或发布后的更新源，无法在当前环境自动完成：

1. macOS Touch ID（未签名 + 未公证；钥匙串授权可能在重装后失效）。
2. macOS CGEventTap 与辅助功能授权引导、覆盖层不抢焦点、双屏/Retina 定位。
3. macOS 空闲锁定与锁屏通知（`CGEventSourceSecondsSinceLastEventType` / NSWorkspace）。
4. macOS DMG 启动、Gatekeeper 右键打开、更新替换后的钥匙串行为。
5. Windows Hello 实际验证弹窗（只能检测可用性；未使用真实 PIN/指纹完成一次解锁）。
6. 全局快捷键在 Windows 锁屏解除后的实际触发（自动化期间系统进入锁屏，无法向应用发送前台快捷键）。
7. 托盘菜单人工点击。
8. 360 / QQ 浏览器调试端口逐版本实测。
9. 真实站点的验证码准确率采样（当前仅 8 张参考图与 fixture）。
10. 更新源真实下载、安装、重启与更新日志弹窗（需要已发布 Release 与签名）。
11. 安全软件/反作弊拦截 WH_MOUSE_LL 的场景。

详见 `docs/manual-test-checklist.md`。

## 10. 复现命令

```powershell
pnpm install
pnpm test
pnpm build
cargo test --workspace
cargo clippy --workspace --all-targets
pnpm tauri build --bundles nsis
```

CDP spike 与 OCR 评估：

```powershell
pnpm fixtures:build
cargo run -p keydock-browser --example cdp_spike
cargo run -p keydock-browser --example captcha_eval -- "F:\Rea_temp\DevDock\captcha-samples"
```
