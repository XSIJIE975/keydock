# ADR 0001: CDP 驱动选型——手写薄客户端

- 状态：Accepted
- 日期：2026-10-20
- 范围：Phase 0 浏览器 spike
- 结论：放弃 `chromiumoxide`，采用 `tokio-tungstenite` + `serde_json` 手写薄 CDP 客户端。

## 背景

交接文档要求先评估 `chromiumoxide`（D17）。只要 §7.2 的四项判据中任一项不满足，就改为手写薄客户端。评估必须用真实 Chrome 跑通，并且不能依赖截图或视觉回归。

Phase 0 使用 Chrome for Windows `155.0.8059.84`，Windows `10.0.26300 x64`，本地双起源 fixture。

## 判据与 chromiumoxide 0.9.1 实测

探针代码：`crates/keydock-browser/examples/chromiumoxide_probe.rs`
探针结果：`F:\Rea_temp\KeyDock\phase0\chromiumoxide-probe.json`（临时目录，不提交）

| # | 判据 | 结果 | 证据 |
|---|---|---|---|
| 1 | `Browser.setDownloadBehavior` + `Browser.downloadProgress` | PARTIAL（API 层） | 生成协议包含 `SetDownloadBehaviorParams`、`EventDownloadProgress`；探针发送命令成功，`browser.event_listener::<EventDownloadProgress>()` 注册成功 |
| 2 | 启动参数完全控制，不做默认覆盖 | **FAIL** | 见下 |
| 3 | `Target.attachToTarget { flatten: true }` 并路由 `sessionId` | PASS（有限） | `handler/target.rs` 内部使用 `flatten(true)` 并维护 session 路由；但没有公开的原始 `sessionId` 控制接口 |
| 4 | 在指定 `executionContextId` 执行脚本 | PASS | `Page::frame_execution_context` / `frame_secondary_execution_context` 可用 |

### 判据 2 的失败证据

即使使用 `disable_default_args()`，并传入 `--window-size=1280,860`：

- 实际页面 `window.innerWidth/innerHeight` 为 **800×600**。
  原因：`BrowserConfig` 默认 `viewport: Some(Viewport::default())`，target 初始化进入
  `TargetInit::InitializingEmulation`，通过 `Emulation.setDeviceMetricsOverride`
  固定视口。这直接违反“不发送 `Emulation.setDeviceMetricsOverride`，页面跟随真实窗口尺寸”。
- 客户端仍强制注入 `--disable-extensions`。
- 参数 API 会对已带 `--` 的字符串再次加前缀，实际命令行为
  `----no-first-run`、`----remote-debugging-port=0`，控制语义不直观。
- 如果使用默认参数路径，还会注入 `--enable-automation`（源码 `DEFAULT_ARGS`）。

`disable_default_args()` 并不能消除强制注入和视口覆盖；若靠修改/包装第三方库绕过，成本高于手写客户端。因此按 D17 的判据立即切换。

## 决定

使用薄客户端，代码位于 `crates/keydock-browser/src/cdp/`：

| 文件 | 作用 |
|---|---|
| `connection.rs` | WebSocket 连接、命令 id 路由、flat `sessionId` 注入、事件广播 |
| `browser.rs` | 严格参数启动、`DevToolsActivePort` 解析、进程命令行核验、关闭与 Profile 删除 |
| `page.rs` | 导航、Runtime 求值、native value setter、trusted 鼠标输入、iframe execution context、下载事件结构 |
| `mod.rs` | 对外导出 |

设计约束：

- 启动参数只包含显式列表；不注入 `--enable-automation`、`--headless`、
  `--disable-extensions` 或视口覆盖。
- 事件在浏览器连接上按 `method` 与可选 `sessionId` 广播，命令与事件解耦。
- 每个页面使用独立的 flat session；iframe 通过
  `Page.getFrameTree` → `Page.createIsolatedWorld` 拿到 `executionContextId`。
  这与交接文档建议的 `DOM.describeNode` 路线不同，但更直接：frame id 已在 frame tree 中，
  不依赖同域 `contentDocument`。
- 浏览器级下载事件不带 `sessionId`，由根连接接收。
- 输入坐标使用 CSS 像素；iframe 内元素坐标由子 frame 矩形加父 frame 的 content 偏移得到。

## Phase 0 流程结果（代表性一轮）

命令：`cargo run -p keydock-browser --example cdp_spike`

| 检查 | 结果 | 耗时 | 关键证据 |
|---|---|---:|---|
| 启动 + DevToolsActivePort + 连接 | PASS | 3079ms | 启动 444–875ms；读到端口与 `/devtools/browser/...` |
| 启动参数核验 | PASS | 0ms | 无 `--enable-automation`、`--headless`、`--disable-extensions` |
| flat session 路由 | PASS | 114ms | 两个 target 各自 session 求值成功 |
| 导航 fixture | PASS | 93ms | URL 与 title 正确 |
| 普通表单填充、trusted 提交、成功选择器 | PASS | 182ms | `isTrusted=true`；`#success=NORMAL_OK` |
| React 19 受控表单 | PASS | 289ms | 原生 value setter 后 React state = `react-user\|10` |
| 跨域 iframe（隔离世界） | PASS | 257ms | contextId=6；iframe 内 `isTrusted=true` |
| 跨域 SSO 成功 | PASS | 452ms | 离开站点源后回到站点 host 才判成功 |
| 密码错误不误判 | PASS | 1223ms | 停留 IdP、`SSO_BAD_CREDENTIALS`、900ms 内未判成功 |
| 下载 | PASS | 411ms | 系统 Downloads；原文件名；114,717 字节；`completed` |
| 延迟渲染 SPA | PASS | 1463ms | 1.2s 后出现表单并完成登录 |
| 窗口缩放重排 | PASS | 959ms | innerWidth 686→1186；media query 跟随；未用 Emulation |
| 关闭与 Profile 删除 | PASS | 1075ms | graceful exit 393ms；未 kill；目录已删除 |

- 临时 Profile 位于 `F:\Rea_temp\KeyDock\phase0\sessions\...`，每轮结束后删除。
- 下载验证文件在检查后删除，未留在用户下载目录。
- 日志中没有写入任何凭据；fixture 使用固定测试值。
- 未使用截图、录屏或屏幕捕获。

## 实现结论

1. `DevToolsActivePort` 在 `--remote-debugging-port=0` 下稳定生成；必须等待文件完整写入，并做连接重试。
2. `Browser.setDownloadBehavior { behavior:"allow", downloadPath, eventsEnabled:true }` 必须在点击下载前发送；完成判定用 `Browser.downloadProgress.state == "completed"`，不用文件存在性。
3. React 受控输入必须用原型上的原生 `value` setter，再派发冒泡的 `input` 与 `change`；直接 `el.value =` 不更新 React state。
4. 跨域 iframe 填充可在 `Page.createIsolatedWorld` 返回的上下文里完成；点击用主 target 的 `Input.dispatchMouseEvent`，坐标由父子偏移合成，实测 `isTrusted=true`。
5. SSO 成功判定不能只看 URL 变化。提交可能发生在 IdP 上，因此从提交时刻起记录“已离开站点源”，只有回到站点 host 且成功条件成立才通过。
6. 不设置 `Emulation.setDeviceMetricsOverride` 时，`Browser.setWindowBounds` 能真实触发页面重排。
7. 关闭顺序：`Browser.close` → 等待进程退出（本次 393ms）→ 删除 Profile。超时才 kill。

## 限制与后续

- 本轮只验证 Chrome 155；Edge、Chromium、Brave、360、QQ 的探测与启动属于 Phase 3/4。
- macOS 未在本环境验证；`DevToolsActivePort`、路径与进程参数需要在 arm64 Mac 上补跑。
- 手写客户端目前只覆盖 Phase 0 所需命令；Phase 3 在其上补：
  `CdpDriver` trait、登录状态机、验证码四模式、Job Object、启动清扫、导航事件跟踪。
- 需要补的稳定性项：命令超时后的 pending 清理、连接关闭错误分类、事件丢弃计数、
  多标签页/弹窗策略、下载冲突文件名的处理。
- 浏览器安全策略造成的下载拦截、企业策略扩展、防病毒拦截需要在真实目标机上验证。

## 复现

```powershell
pnpm fixtures:build
cargo run -p keydock-browser --example chromiumoxide_probe
cargo run -p keydock-browser --example cdp_spike
```

报告默认写到 `F:\Rea_temp\KeyDock\phase0\`，可用 `KEYDOCK_PHASE0_DIR` 覆盖。
